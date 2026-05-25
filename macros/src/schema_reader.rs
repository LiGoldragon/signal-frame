//! Schema crate adapter for `signal_channel!([schema])`.
//!
//! The schema language is parsed and assembled by the `schema` crate.
//! This module only translates the assembled signal shape into the
//! existing `signal_channel!` emission model.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use proc_macro2::Span;
use schema::{
    AssembledSchema, AssembledType, Container, DeclarationBody, Feature, ImportDirective, Leg,
    LoadedSchema, Name, Payload, Primitive, RouteBody, TypeExpression,
};
use syn::{Ident, Type, parse_quote};

use crate::model::{
    ChannelSpec, EventBlockSpec, EventVariantSpec, FilterDecl, ObservableBlockSpec, ReplyBlockSpec,
    ReplyVariantSpec, RequestBlockSpec, RequestVariantSpec, SchemaDefinition, SchemaField,
    SchemaSpec, SchemaType, SchemaVariant, StreamBlockSpec,
};

pub(crate) fn read_default_schema() -> syn::Result<ChannelSpec> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").map_err(|error| {
        syn::Error::new(
            Span::call_site(),
            format!("cannot read CARGO_MANIFEST_DIR for schema file: {error}"),
        )
    })?;
    let root = PathBuf::from(manifest);
    let path = default_schema_path(&root);
    let loaded = LoadedSchema::read_path(&path).map_err(|error| schema_error(&path, error))?;
    SchemaConverter::new(&loaded).into_channel_spec()
}

fn default_schema_path(root: &Path) -> PathBuf {
    let package = std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "signal".to_string());
    let stem = schema_file_stem_from_package(&package);
    let schema_path = root.join(format!("{stem}.schema"));
    if schema_path.exists() {
        return schema_path;
    }

    root.join("schema.nota")
}

fn schema_file_stem_from_package(package: &str) -> String {
    package
        .strip_prefix("owner-signal-persona-")
        .or_else(|| package.strip_prefix("signal-persona-"))
        .or_else(|| package.strip_prefix("owner-signal-"))
        .or_else(|| package.strip_prefix("signal-"))
        .unwrap_or(package)
        .to_string()
}

struct SchemaConverter<'schema> {
    loaded: &'schema LoadedSchema,
    assembled: &'schema AssembledSchema,
}

impl<'schema> SchemaConverter<'schema> {
    fn new(loaded: &'schema LoadedSchema) -> Self {
        Self {
            loaded,
            assembled: loaded.assembled(),
        }
    }

    fn into_channel_spec(self) -> syn::Result<ChannelSpec> {
        let name = channel_name_from_manifest();
        let request = self.request_block()?;
        let reply = self.reply_block()?;
        let event = self.event_block()?;
        let streams = self.stream_blocks(&request, event.as_ref())?;
        let request = request_with_open_streams(request, &streams);
        let observable = self.observable_block()?;
        let schema = Some(self.schema_spec()?);

        Ok(ChannelSpec {
            name,
            request,
            reply,
            event,
            streams,
            observable,
            schema,
        })
    }

    fn request_block(&self) -> syn::Result<RequestBlockSpec> {
        let mut seen_roots = BTreeSet::new();
        let mut variants = Vec::new();

        for route in self
            .assembled
            .routes()
            .iter()
            .filter(|route| route.leg() == Leg::Ordinary)
        {
            let root = route.root().as_str().to_string();
            if !seen_roots.insert(root.clone()) {
                return Err(syn::Error::new(
                    Span::call_site(),
                    format!(
                        "schema macro MVP supports one endpoint per operation root; `{root}` has multiple endpoints"
                    ),
                ));
            }

            variants.push(RequestVariantSpec {
                variant_name: ident(&root),
                payload_type: route_body_type(route.body())?,
                opens: None,
            });
        }

        Ok(RequestBlockSpec {
            name: ident("Operation"),
            variants,
        })
    }

    fn reply_block(&self) -> syn::Result<ReplyBlockSpec> {
        let replies = self.reply_names()?;
        Ok(ReplyBlockSpec {
            name: ident("Reply"),
            variants: replies
                .iter()
                .map(|name| ReplyVariantSpec {
                    variant_name: ident(name.as_str()),
                    payload_type: named_type(name.as_str()),
                })
                .collect(),
        })
    }

    fn event_block(&self) -> syn::Result<Option<EventBlockSpec>> {
        let event = self
            .assembled
            .features()
            .iter()
            .find_map(|feature| match feature {
                Feature::Event(event) => Some(event),
                _ => None,
            });

        let Some(event) = event else {
            return Ok(None);
        };

        let belongs = event.stream().map(|stream| ident(stream.as_str()));
        Ok(Some(EventBlockSpec {
            name: ident("Event"),
            variants: event
                .events()
                .iter()
                .map(|name| EventVariantSpec {
                    variant_name: ident(name.as_str()),
                    payload_type: named_type(name.as_str()),
                    belongs: belongs.clone(),
                })
                .collect(),
        }))
    }

    fn observable_block(&self) -> syn::Result<Option<ObservableBlockSpec>> {
        let mut observable = None;
        for feature in self.assembled.features() {
            let Feature::Observable(candidate) = feature else {
                continue;
            };
            if observable.replace(candidate).is_some() {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "schema declares more than one Observable feature",
                ));
            }
        }

        let Some(observable) = observable else {
            return Ok(None);
        };

        let filter = match observable.filter() {
            None | Some("default") => FilterDecl::Default,
            Some(name) => FilterDecl::Named(ident(name)),
        };
        let operation_event = observable.operation_event().ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "Observable feature must declare operation_event",
            )
        })?;
        let effect_event = observable.effect_event().ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "Observable feature must declare effect_event",
            )
        })?;

        Ok(Some(ObservableBlockSpec {
            filter,
            operation_event: ident(operation_event.as_str()),
            effect_event: ident(effect_event.as_str()),
        }))
    }

    fn reply_names(&self) -> syn::Result<&[Name]> {
        self.assembled
            .features()
            .iter()
            .find_map(|feature| match feature {
                Feature::Reply(names) => Some(names.as_slice()),
                _ => None,
            })
            .ok_or_else(|| syn::Error::new(Span::call_site(), "schema is missing Reply feature"))
    }

    fn stream_blocks(
        &self,
        request: &RequestBlockSpec,
        event: Option<&EventBlockSpec>,
    ) -> syn::Result<Vec<StreamBlockSpec>> {
        let Some(event) = event else {
            return Ok(Vec::new());
        };

        let mut by_stream: BTreeMap<String, Vec<Ident>> = BTreeMap::new();
        for variant in &event.variants {
            let Some(stream) = &variant.belongs else {
                continue;
            };
            by_stream
                .entry(stream.to_string())
                .or_default()
                .push(variant.variant_name.clone());
        }
        if by_stream.is_empty() {
            return Ok(Vec::new());
        }
        if by_stream.len() > 1 {
            return Err(syn::Error::new(
                Span::call_site(),
                "schema macro MVP supports one domain stream per channel",
            ));
        }
        if !request
            .variants
            .iter()
            .any(|variant| variant.variant_name == "Unwatch")
        {
            return Err(syn::Error::new(
                Span::call_site(),
                "streaming schema requires an `Unwatch` operation for the MVP close verb",
            ));
        }

        Ok(by_stream
            .into_iter()
            .map(|(stream, events)| StreamBlockSpec {
                name: ident(&stream),
                token: parse_quote!(SubscriptionToken),
                opened: ident("SubscriptionOpened"),
                events,
                close: ident("Unwatch"),
            })
            .collect())
    }

    fn schema_spec(&self) -> syn::Result<SchemaSpec> {
        let mut definitions = BTreeMap::new();
        collect_imported_definitions(self.loaded, &mut definitions, &mut BTreeSet::new())?;

        for schema_type in self.assembled.types() {
            if let AssembledType::Local { name, body } = schema_type {
                definitions.insert(name.as_str().to_string(), definition_from_body(name, body)?);
            }
        }

        definitions.insert("Operation".into(), self.operation_definition()?);
        definitions.insert("Reply".into(), self.reply_definition()?);
        if let Some(event) = self.event_definition()? {
            definitions.insert("Event".into(), event);
        }

        Ok(SchemaSpec { definitions })
    }

    fn operation_definition(&self) -> syn::Result<SchemaDefinition> {
        let mut variants = Vec::new();
        let mut seen_roots = BTreeSet::new();
        for route in self
            .assembled
            .routes()
            .iter()
            .filter(|route| route.leg() == Leg::Ordinary)
        {
            let root = route.root().as_str().to_string();
            if !seen_roots.insert(root.clone()) {
                continue;
            }
            variants.push(SchemaVariant::Data {
                name: root,
                fields: vec![SchemaField::inferred(schema_type_from_route_body(
                    route.body(),
                )?)],
            });
        }
        Ok(SchemaDefinition {
            name: "Operation".into(),
            variants,
            alias: None,
        })
    }

    fn reply_definition(&self) -> syn::Result<SchemaDefinition> {
        Ok(SchemaDefinition {
            name: "Reply".into(),
            variants: self
                .reply_names()?
                .iter()
                .map(|name| SchemaVariant::Data {
                    name: name.as_str().to_string(),
                    fields: vec![SchemaField::inferred(SchemaType::Named(
                        name.as_str().to_string(),
                    ))],
                })
                .collect(),
            alias: None,
        })
    }

    fn event_definition(&self) -> syn::Result<Option<SchemaDefinition>> {
        let Some(event) = self
            .assembled
            .features()
            .iter()
            .find_map(|feature| match feature {
                Feature::Event(event) => Some(event),
                _ => None,
            })
        else {
            return Ok(None);
        };

        Ok(Some(SchemaDefinition {
            name: "Event".into(),
            variants: event
                .events()
                .iter()
                .map(|name| SchemaVariant::Data {
                    name: name.as_str().to_string(),
                    fields: vec![SchemaField::inferred(SchemaType::Named(
                        name.as_str().to_string(),
                    ))],
                })
                .collect(),
            alias: None,
        }))
    }
}

fn collect_imported_definitions(
    loaded: &LoadedSchema,
    definitions: &mut BTreeMap<String, SchemaDefinition>,
    visiting: &mut BTreeSet<PathBuf>,
) -> syn::Result<()> {
    for (_binding, directive) in loaded.schema().imports().entries() {
        let path = resolve_import_path(loaded.path(), directive.path().as_str())?;
        if !visiting.insert(path.clone()) {
            continue;
        }
        let imported =
            LoadedSchema::read_path(&path).map_err(|error| schema_error(&path, error))?;
        collect_imported_definitions(&imported, definitions, visiting)?;

        let selected = match directive {
            ImportDirective::Import { names, .. } => Some(
                names
                    .iter()
                    .map(|name| name.as_str().to_string())
                    .collect::<BTreeSet<_>>(),
            ),
            ImportDirective::ImportAll { .. } => None,
        };

        for schema_type in imported.assembled().types() {
            let AssembledType::Local { name, body } = schema_type else {
                continue;
            };
            if selected
                .as_ref()
                .is_some_and(|names| !names.contains(name.as_str()))
            {
                continue;
            }
            definitions
                .entry(name.as_str().to_string())
                .or_insert(definition_from_body(name, body)?);
        }
        visiting.remove(&path);
    }
    Ok(())
}

fn resolve_import_path(source: &Path, import: &str) -> syn::Result<PathBuf> {
    let import_path = Path::new(import);
    let path = if import_path.is_absolute() {
        import_path.to_path_buf()
    } else {
        source
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(import_path)
    };
    path.canonicalize().map_err(|error| {
        syn::Error::new(
            Span::call_site(),
            format!("cannot resolve schema import `{import}`: {error}"),
        )
    })
}

fn definition_from_body(name: &Name, body: &DeclarationBody) -> syn::Result<SchemaDefinition> {
    let name_text = name.as_str().to_string();
    match body {
        DeclarationBody::Enum { variants } => Ok(SchemaDefinition {
            name: name_text,
            variants: variants
                .iter()
                .map(|variant| match variant.payload() {
                    Payload::Unit => Ok(SchemaVariant::Unit {
                        name: variant.name().as_str().to_string(),
                    }),
                    Payload::Type(expression) => Ok(SchemaVariant::Data {
                        name: variant.name().as_str().to_string(),
                        fields: vec![SchemaField::inferred(schema_type_from_expression(
                            expression,
                        )?)],
                    }),
                    Payload::Fields(fields) => Ok(SchemaVariant::Data {
                        name: variant.name().as_str().to_string(),
                        fields: fields
                            .iter()
                            .map(schema_field_from_schema_field)
                            .collect::<syn::Result<Vec<_>>>()?,
                    }),
                })
                .collect::<syn::Result<Vec<_>>>()?,
            alias: None,
        }),
        DeclarationBody::Newtype(expression) | DeclarationBody::Alias(expression) => {
            Ok(SchemaDefinition {
                name: name_text,
                variants: Vec::new(),
                alias: Some(schema_type_from_expression(expression)?),
            })
        }
        DeclarationBody::Record(fields) => Ok(SchemaDefinition {
            name: name_text.clone(),
            variants: vec![SchemaVariant::Data {
                name: name_text,
                fields: fields
                    .iter()
                    .map(schema_field_from_schema_field)
                    .collect::<syn::Result<Vec<_>>>()?,
            }],
            alias: None,
        }),
    }
}

fn route_body_type(body: &RouteBody) -> syn::Result<Type> {
    match body {
        RouteBody::Type(name) => Ok(named_type(name.as_str())),
        RouteBody::Unit => Err(syn::Error::new(
            Span::call_site(),
            "schema macro MVP does not emit unit operation payloads yet",
        )),
    }
}

fn schema_type_from_route_body(body: &RouteBody) -> syn::Result<SchemaType> {
    match body {
        RouteBody::Type(name) => Ok(SchemaType::Named(name.as_str().to_string())),
        RouteBody::Unit => Err(syn::Error::new(
            Span::call_site(),
            "schema macro MVP does not model unit operation payloads yet",
        )),
    }
}

fn schema_type_from_expression(expression: &TypeExpression) -> syn::Result<SchemaType> {
    match expression {
        TypeExpression::Primitive(primitive) => Ok(SchemaType::Named(primitive_name(*primitive))),
        TypeExpression::Named(name) => Ok(SchemaType::Named(name.as_str().to_string())),
        TypeExpression::Container(Container::Vector(inner)) => Ok(SchemaType::Vec(Box::new(
            schema_type_from_expression(inner)?,
        ))),
        TypeExpression::Container(Container::Optional(inner)) => Ok(SchemaType::Option(Box::new(
            schema_type_from_expression(inner)?,
        ))),
        TypeExpression::Container(Container::Map { .. }) => Err(syn::Error::new(
            Span::call_site(),
            "schema macro MVP does not lower Map container expressions",
        )),
    }
}

fn schema_field_from_schema_field(field: &schema::Field) -> syn::Result<SchemaField> {
    let schema_type = schema_type_from_expression(field.expression())?;
    Ok(match field.name() {
        Some(name) => SchemaField::named(name.as_str(), schema_type),
        None => SchemaField::inferred(schema_type),
    })
}

fn primitive_name(primitive: Primitive) -> String {
    match primitive {
        Primitive::String => "String",
        Primitive::Bytes => "Bytes",
        Primitive::Boolean => "bool",
        Primitive::Unsigned8 => "u8",
        Primitive::Unsigned16 => "u16",
        Primitive::Unsigned32 => "u32",
        Primitive::Unsigned64 => "u64",
        Primitive::Date => "Date",
        Primitive::Time => "Time",
    }
    .to_string()
}

fn request_with_open_streams(
    mut request: RequestBlockSpec,
    streams: &[StreamBlockSpec],
) -> RequestBlockSpec {
    if streams.len() == 1 {
        let stream = streams[0].name.clone();
        for variant in &mut request.variants {
            if variant.variant_name == "Watch" {
                variant.opens = Some(stream.clone());
            }
        }
    }
    request
}

fn named_type(name: &str) -> Type {
    let ident = ident(name);
    parse_quote!(#ident)
}

fn channel_name_from_manifest() -> Ident {
    let package = std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "Signal".to_string());
    let stem = package
        .strip_prefix("owner-signal-persona-")
        .or_else(|| package.strip_prefix("signal-persona-"))
        .or_else(|| package.strip_prefix("owner-signal-"))
        .or_else(|| package.strip_prefix("signal-"))
        .unwrap_or(&package);
    ident(&to_pascal_case(stem))
}

fn ident(name: &str) -> Ident {
    Ident::new(name, Span::call_site())
}

fn to_pascal_case(value: &str) -> String {
    let mut result = String::new();
    let mut uppercase = true;
    for character in value.chars() {
        if character == '-' || character == '_' {
            uppercase = true;
        } else if uppercase {
            result.extend(character.to_uppercase());
            uppercase = false;
        } else {
            result.push(character);
        }
    }
    result
}

fn schema_error(path: &Path, error: schema::Error) -> syn::Error {
    syn::Error::new(
        Span::call_site(),
        format!("cannot read schema file {}: {error}", path.display()),
    )
}
