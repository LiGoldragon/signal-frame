//! Rust composer for assembled Persona schemas.
//!
//! This crate is deliberately outside the proc-macro crate so the
//! schema-to-Rust decision tree can be tested as ordinary Rust. The
//! proc macro is only the token front door.

use std::fmt;

use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
use schema::{
    AssembledSchema, AssembledType, Container, DeclarationBody, Feature, Field, Leg, LoadedSchema,
    Name, Payload, Primitive, Route, RouteBody, TypeExpression, Variant,
};

#[derive(Debug)]
pub struct ComposerError {
    message: String,
}

impl ComposerError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ComposerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for ComposerError {}

pub type Result<T> = std::result::Result<T, ComposerError>;

#[derive(Clone, Debug)]
pub struct RustModule {
    name: String,
    tokens: TokenStream,
}

impl RustModule {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn into_tokens(self) -> TokenStream {
        self.tokens
    }
}

pub struct RustComposer<'schema> {
    loaded: &'schema LoadedSchema,
}

impl<'schema> RustComposer<'schema> {
    pub fn new(loaded: &'schema LoadedSchema) -> Self {
        Self { loaded }
    }

    pub fn compose(&self) -> Result<RustModule> {
        let module_name = module_name_for_schema(self.loaded)?;
        let module_ident = format_ident!("{module_name}");
        let schema_path = self.loaded.path().display().to_string();
        let assembled = self.loaded.assembled();
        let type_items = self.type_items(assembled)?;
        let route_items = self.route_items(assembled)?;
        let operation_items = self.operation_items(assembled, Leg::Ordinary, "Operation")?;
        let owner_operation_items =
            self.operation_items(assembled, Leg::Owner, "OwnerOperation")?;
        let sema_operation_items = self.operation_items(assembled, Leg::Sema, "SemaOperation")?;
        let reply_items = self.reply_items(assembled)?;
        let event_items = self.event_items(assembled)?;

        let tokens = quote! {
            pub mod #module_ident {
                pub const SCHEMA_PATH: &str = #schema_path;

                #(#type_items)*
                #(#operation_items)*
                #(#owner_operation_items)*
                #(#sema_operation_items)*
                #(#reply_items)*
                #(#event_items)*
                #route_items
            }
        };

        Ok(RustModule {
            name: module_name,
            tokens,
        })
    }

    fn type_items(&self, assembled: &AssembledSchema) -> Result<Vec<TokenStream>> {
        assembled
            .types()
            .map(|schema_type| self.type_item(schema_type))
            .collect()
    }

    fn type_item(&self, schema_type: &AssembledType) -> Result<TokenStream> {
        match schema_type {
            AssembledType::Local { name, body } => self.local_type_item(name, body),
            AssembledType::Imported { name, .. } => {
                let ident = type_ident(name);
                Ok(quote! {
                    #[derive(Clone, Debug, PartialEq, Eq)]
                    pub struct #ident;
                })
            }
        }
    }

    fn local_type_item(&self, name: &Name, body: &DeclarationBody) -> Result<TokenStream> {
        let ident = type_ident(name);
        match body {
            DeclarationBody::Enum { variants } => {
                let variants = variants
                    .iter()
                    .map(enum_variant_tokens)
                    .collect::<Result<Vec<_>>>()?;
                Ok(quote! {
                    #[derive(Clone, Debug, PartialEq, Eq)]
                    pub enum #ident {
                        #(#variants),*
                    }
                })
            }
            DeclarationBody::Newtype(expression) => {
                let inner = type_expression_tokens(expression)?;
                Ok(quote! {
                    #[derive(Clone, Debug, PartialEq, Eq)]
                    pub struct #ident(pub #inner);
                })
            }
            DeclarationBody::Record(fields) => {
                let fields = fields
                    .iter()
                    .map(record_field_tokens)
                    .collect::<Result<Vec<_>>>()?;
                Ok(quote! {
                    #[derive(Clone, Debug, PartialEq, Eq)]
                    pub struct #ident {
                        #(#fields),*
                    }
                })
            }
            DeclarationBody::Alias(expression) => {
                let target = type_expression_tokens(expression)?;
                Ok(quote! {
                    pub type #ident = #target;
                })
            }
        }
    }

    fn operation_items(
        &self,
        assembled: &AssembledSchema,
        leg: Leg,
        operation_name: &str,
    ) -> Result<Vec<TokenStream>> {
        let routes = routes_for_leg(assembled, leg);
        if routes.is_empty() {
            return Ok(Vec::new());
        }

        let operation_ident = format_ident!("{operation_name}");
        let root_items = root_endpoint_items(&routes)?;
        let operation_variants = routes
            .iter()
            .map(|root| {
                let root_ident = type_ident(root.root);
                let endpoint_ident = endpoint_enum_ident(root.root);
                quote! { #root_ident(#endpoint_ident) }
            })
            .collect::<Vec<_>>();
        let log_arms = routes
            .iter()
            .map(|root| {
                let root_ident = type_ident(root.root);
                let root_slot = u8_literal(root.root_slot, root.root)?;
                Ok(quote! {
                    Self::#root_ident(endpoint) => {
                        let endpoint_slot = endpoint.endpoint_slot();
                        u64::from_le_bytes([#root_slot, endpoint_slot, 0, 0, 0, 0, 0, 0])
                    }
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let mut items = root_items;
        items.push(quote! {
            #[derive(Clone, Debug, PartialEq, Eq)]
            pub enum #operation_ident {
                #(#operation_variants),*
            }

            impl signal_frame::LogVariant for #operation_ident {
                fn log_variant(&self) -> u64 {
                    match self {
                        #(#log_arms),*
                    }
                }
            }
        });
        Ok(items)
    }

    fn reply_items(&self, assembled: &AssembledSchema) -> Result<Vec<TokenStream>> {
        let Some(replies) = assembled
            .features()
            .iter()
            .find_map(|feature| match feature {
                Feature::Reply(replies) => Some(replies),
                _ => None,
            })
        else {
            return Ok(Vec::new());
        };

        let variants = replies
            .iter()
            .map(|name| {
                let ident = type_ident(name);
                quote! { #ident(#ident) }
            })
            .collect::<Vec<_>>();

        Ok(vec![quote! {
            #[derive(Clone, Debug, PartialEq, Eq)]
            pub enum Reply {
                #(#variants),*
            }
        }])
    }

    fn event_items(&self, assembled: &AssembledSchema) -> Result<Vec<TokenStream>> {
        let Some(event) = assembled
            .features()
            .iter()
            .find_map(|feature| match feature {
                Feature::Event(event) => Some(event),
                _ => None,
            })
        else {
            return Ok(Vec::new());
        };

        let variants = event
            .events()
            .iter()
            .map(|name| {
                let ident = type_ident(name);
                quote! { #ident(#ident) }
            })
            .collect::<Vec<_>>();

        Ok(vec![quote! {
            #[derive(Clone, Debug, PartialEq, Eq)]
            pub enum Event {
                #(#variants),*
            }
        }])
    }

    fn route_items(&self, assembled: &AssembledSchema) -> Result<TokenStream> {
        let route_literals = assembled
            .routes()
            .iter()
            .map(route_descriptor_tokens)
            .collect::<Result<Vec<_>>>()?;
        let route_match_arms = assembled
            .routes()
            .iter()
            .enumerate()
            .map(|(index, route)| route_match_arm_tokens(index, route))
            .collect::<Result<Vec<_>>>()?;
        let route_count = route_literals.len();

        Ok(quote! {
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub enum Leg {
                Ordinary,
                Owner,
                Sema,
            }

            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub enum RouteBodyDescriptor {
                Unit,
                Type(&'static str),
            }

            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub struct RouteDescriptor {
                pub leg: Leg,
                pub root_slot: u8,
                pub endpoint_slot: u8,
                pub root: &'static str,
                pub endpoint: &'static str,
                pub body: RouteBodyDescriptor,
            }

            pub const ROUTE_COUNT: usize = #route_count;

            pub const ROUTES: &[RouteDescriptor] = &[
                #(#route_literals),*
            ];

            pub fn route_for_short_header(
                leg: Leg,
                header: signal_frame::ShortHeader,
            ) -> Option<RouteDescriptor> {
                let bytes = header.to_le_bytes();
                match (leg, bytes[0], bytes[1]) {
                    #(#route_match_arms),*,
                    _ => None,
                }
            }
        })
    }
}

fn module_name_for_schema(loaded: &LoadedSchema) -> Result<String> {
    let stem = loaded
        .path()
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| ComposerError::new("schema path has no UTF-8 file stem"))?;
    let name = stem.replace('-', "_");
    if name
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        Ok(name)
    } else {
        Err(ComposerError::new(format!(
            "schema file stem `{stem}` cannot become a Rust module name"
        )))
    }
}

fn routes_for_leg(assembled: &AssembledSchema, leg: Leg) -> Vec<RootRoutes<'_>> {
    let mut roots = Vec::<RootRoutes<'_>>::new();
    for route in assembled.routes().iter().filter(|route| route.leg() == leg) {
        if let Some(root) = roots
            .iter_mut()
            .find(|candidate| candidate.root == route.root())
        {
            root.routes.push(route);
        } else {
            roots.push(RootRoutes {
                root: route.root(),
                root_slot: route.root_slot(),
                routes: vec![route],
            });
        }
    }
    roots
}

struct RootRoutes<'schema> {
    root: &'schema Name,
    root_slot: usize,
    routes: Vec<&'schema Route>,
}

fn root_endpoint_items(roots: &[RootRoutes<'_>]) -> Result<Vec<TokenStream>> {
    roots.iter().map(root_endpoint_item).collect()
}

fn root_endpoint_item(root: &RootRoutes<'_>) -> Result<TokenStream> {
    let endpoint_ident = endpoint_enum_ident(root.root);
    let endpoint_variants = root
        .routes
        .iter()
        .map(|route| endpoint_variant_tokens(route.endpoint().name(), route.body()))
        .collect::<Result<Vec<_>>>()?;
    let slot_arms = root
        .routes
        .iter()
        .map(|route| {
            let endpoint_ident = type_ident(route.endpoint().name());
            let slot = u8_literal(route.endpoint().slot(), route.endpoint().name())?;
            let pattern = match route.body() {
                RouteBody::Unit => quote! { Self::#endpoint_ident },
                RouteBody::Type(_) => quote! { Self::#endpoint_ident(_) },
            };
            Ok(quote! { #pattern => #slot })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum #endpoint_ident {
            #(#endpoint_variants),*
        }

        impl #endpoint_ident {
            pub const fn endpoint_slot(&self) -> u8 {
                match self {
                    #(#slot_arms),*
                }
            }
        }
    })
}

fn enum_variant_tokens(variant: &Variant) -> Result<TokenStream> {
    let ident = type_ident(variant.name());
    match variant.payload() {
        Payload::Unit => Ok(quote! { #ident }),
        Payload::Type(expression) => {
            let payload = type_expression_tokens(expression)?;
            Ok(quote! { #ident(#payload) })
        }
        Payload::Fields(fields) => {
            let fields = fields
                .iter()
                .map(record_field_tokens)
                .collect::<Result<Vec<_>>>()?;
            Ok(quote! { #ident { #(#fields),* } })
        }
    }
}

fn endpoint_variant_tokens(endpoint: &Name, body: &RouteBody) -> Result<TokenStream> {
    let ident = type_ident(endpoint);
    match body {
        RouteBody::Unit => Ok(quote! { #ident }),
        RouteBody::Type(name) => {
            let payload = type_ident(name);
            Ok(quote! { #ident(#payload) })
        }
    }
}

fn record_field_tokens(field: &Field) -> Result<TokenStream> {
    let name = field.effective_name();
    let ident = Ident::new(name.as_str(), Span::call_site());
    let expression = type_expression_tokens(field.expression())?;
    Ok(quote! { pub #ident: #expression })
}

fn type_expression_tokens(expression: &TypeExpression) -> Result<TokenStream> {
    match expression {
        TypeExpression::Primitive(primitive) => Ok(primitive_tokens(*primitive)),
        TypeExpression::Named(name) => {
            let ident = type_ident(name);
            Ok(quote! { #ident })
        }
        TypeExpression::Container(Container::Vector(inner)) => {
            let inner = type_expression_tokens(inner)?;
            Ok(quote! { Vec<#inner> })
        }
        TypeExpression::Container(Container::Optional(inner)) => {
            let inner = type_expression_tokens(inner)?;
            Ok(quote! { Option<#inner> })
        }
        TypeExpression::Container(Container::Map { key, value }) => {
            let key = type_expression_tokens(key)?;
            let value = type_expression_tokens(value)?;
            Ok(quote! { ::std::collections::BTreeMap<#key, #value> })
        }
    }
}

fn primitive_tokens(primitive: Primitive) -> TokenStream {
    match primitive {
        Primitive::String => quote! { String },
        Primitive::Bytes => quote! { Vec<u8> },
        Primitive::Boolean => quote! { bool },
        Primitive::Unsigned8 => quote! { u8 },
        Primitive::Unsigned16 => quote! { u16 },
        Primitive::Unsigned32 => quote! { u32 },
        Primitive::Unsigned64 => quote! { u64 },
        Primitive::Date | Primitive::Time => quote! { String },
    }
}

fn route_descriptor_tokens(route: &Route) -> Result<TokenStream> {
    let leg = leg_tokens(route.leg());
    let root_slot = u8_literal(route.root_slot(), route.root())?;
    let endpoint_slot = u8_literal(route.endpoint().slot(), route.endpoint().name())?;
    let root = route.root().as_str();
    let endpoint = route.endpoint().name().as_str();
    let body = route_body_descriptor_tokens(route.body());
    Ok(quote! {
        RouteDescriptor {
            leg: #leg,
            root_slot: #root_slot,
            endpoint_slot: #endpoint_slot,
            root: #root,
            endpoint: #endpoint,
            body: #body,
        }
    })
}

fn route_match_arm_tokens(index: usize, route: &Route) -> Result<TokenStream> {
    let leg = leg_tokens(route.leg());
    let root_slot = u8_literal(route.root_slot(), route.root())?;
    let endpoint_slot = u8_literal(route.endpoint().slot(), route.endpoint().name())?;
    let index = proc_macro2::Literal::usize_unsuffixed(index);
    Ok(quote! { (#leg, #root_slot, #endpoint_slot) => Some(ROUTES[#index]) })
}

fn route_body_descriptor_tokens(body: &RouteBody) -> TokenStream {
    match body {
        RouteBody::Unit => quote! { RouteBodyDescriptor::Unit },
        RouteBody::Type(name) => {
            let name = name.as_str();
            quote! { RouteBodyDescriptor::Type(#name) }
        }
    }
}

fn leg_tokens(leg: Leg) -> TokenStream {
    match leg {
        Leg::Ordinary => quote! { Leg::Ordinary },
        Leg::Owner => quote! { Leg::Owner },
        Leg::Sema => quote! { Leg::Sema },
    }
}

fn u8_literal(value: usize, name: &Name) -> Result<proc_macro2::Literal> {
    let value = u8::try_from(value).map_err(|_| {
        ComposerError::new(format!(
            "schema route `{}` uses slot {value}, which does not fit the 64-bit header byte",
            name.as_str()
        ))
    })?;
    Ok(proc_macro2::Literal::u8_unsuffixed(value))
}

fn type_ident(name: &Name) -> Ident {
    format_ident!("{}", name.as_str())
}

fn endpoint_enum_ident(name: &Name) -> Ident {
    format_ident!("{}Endpoint", name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composer_emits_module_from_assembled_schema_without_legacy_model() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple.schema");
        let loaded = LoadedSchema::read_path(path).expect("schema reads");
        let module = RustComposer::new(&loaded).compose().expect("composes");
        let text = module.into_tokens().to_string();

        assert!(text.contains("pub mod simple"));
        assert!(text.contains("pub enum Operation"));
        assert!(text.contains("pub enum StateEndpoint"));
        assert!(text.contains("pub const ROUTES"));
    }
}
