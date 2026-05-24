//! NOTA schema reader for `signal_channel!([schema])`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use nota_codec::{Decoder, Token};
use proc_macro2::Span;
use syn::{Ident, Type, parse_quote};

use crate::model::{
    ChannelSpec, EventBlockSpec, EventVariantSpec, FilterDecl, ObservableBlockSpec, ReplyBlockSpec,
    ReplyVariantSpec, RequestBlockSpec, RequestVariantSpec, SchemaDefinition, SchemaSpec,
    SchemaType, SchemaVariant, StreamBlockSpec,
};

pub(crate) fn read_default_schema() -> syn::Result<ChannelSpec> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").map_err(|error| {
        syn::Error::new(
            Span::call_site(),
            format!("cannot read CARGO_MANIFEST_DIR for schema.nota: {error}"),
        )
    })?;
    // DESIGN-DECISION-REVIEW (designer/320 §2.3): schema.nota at
    // repo root, not src/. Alternative: src/schema.nota. Revisit if
    // Cargo.toml manifest discovery becomes complicated.
    let root = PathBuf::from(manifest);
    let path = root.join("schema.nota");
    let schema = SchemaReader::new(root).read_schema(&path)?;
    schema.into_channel_spec()
}

struct SchemaReader {
    crate_root: PathBuf,
}

impl SchemaReader {
    fn new(crate_root: PathBuf) -> Self {
        Self { crate_root }
    }

    fn read_schema(&self, path: &Path) -> syn::Result<ResolvedSchema> {
        let text = fs::read_to_string(path).map_err(|error| {
            syn::Error::new(
                Span::call_site(),
                format!("cannot read schema file {}: {error}", path.display()),
            )
        })?;
        let mut parser = SchemaParser::new(&text);
        let mut schema = parser.parse()?;
        self.resolve_path_refs(path.parent().unwrap_or(&self.crate_root), &mut schema)?;
        Ok(schema)
    }

    fn resolve_path_refs(&self, base: &Path, schema: &mut ResolvedSchema) -> syn::Result<()> {
        let refs: Vec<(String, String)> = schema
            .definitions
            .iter()
            .filter_map(|(name, definition)| {
                definition
                    .path_ref
                    .as_ref()
                    .map(|path| (name.clone(), path.clone()))
            })
            .collect();

        for (name, path_ref) in refs {
            // DESIGN-DECISION-REVIEW (designer/320 §2.7): sandboxed
            // path-ref resolution. Alternative: arbitrary file system
            // traversal. Revisit if cross-crate symbolic refs prove too
            // restrictive for a legitimate use case.
            let path = self.resolve_path_ref(base, &path_ref)?;
            let external = self.read_schema(&path)?;
            let replacement = external.definitions.get(&name).cloned().ok_or_else(|| {
                syn::Error::new(
                    Span::call_site(),
                    format!("schema path-ref `{path_ref}` did not define requested enum `{name}`"),
                )
            })?;
            schema.definitions.insert(name, replacement);
        }
        Ok(())
    }

    fn resolve_path_ref(&self, base: &Path, path_ref: &str) -> syn::Result<PathBuf> {
        if path_ref.starts_with('/') || path_ref.starts_with("~/") {
            return Err(syn::Error::new(
                Span::call_site(),
                format!("schema path-ref `{path_ref}` escapes the crate sandbox"),
            ));
        }

        let candidate = base.join(path_ref);
        let canonical = candidate.canonicalize().map_err(|error| {
            syn::Error::new(
                Span::call_site(),
                format!("cannot resolve schema path-ref `{path_ref}`: {error}"),
            )
        })?;
        let crate_root = self.crate_root.canonicalize().map_err(|error| {
            syn::Error::new(
                Span::call_site(),
                format!(
                    "cannot canonicalize crate root {}: {error}",
                    self.crate_root.display()
                ),
            )
        })?;

        if canonical.starts_with(&crate_root) {
            return Ok(canonical);
        }

        let Some(workspace_root) = crate_root.parent() else {
            return Err(syn::Error::new(
                Span::call_site(),
                format!("schema path-ref `{path_ref}` has no workspace parent"),
            ));
        };
        if canonical.parent().and_then(|parent| parent.parent()) == Some(workspace_root) {
            return Ok(canonical);
        }

        Err(syn::Error::new(
            Span::call_site(),
            format!("schema path-ref `{path_ref}` escapes the crate sandbox"),
        ))
    }
}

struct SchemaParser<'input> {
    decoder: Decoder<'input>,
}

impl<'input> SchemaParser<'input> {
    fn new(text: &'input str) -> Self {
        Self {
            decoder: Decoder::new(text),
        }
    }

    fn parse(&mut self) -> syn::Result<ResolvedSchema> {
        self.decoder.expect_seq_start().map_err(to_syn_error)?;
        let mut definitions = BTreeMap::new();
        while !self.decoder.peek_is_seq_end().map_err(to_syn_error)? {
            let definition = self.parse_definition()?;
            definitions.insert(definition.name.clone(), definition);
        }
        self.decoder.expect_seq_end().map_err(to_syn_error)?;
        Ok(ResolvedSchema { definitions })
    }

    fn parse_definition(&mut self) -> syn::Result<ResolvedDefinition> {
        self.decoder.expect_record_start().map_err(to_syn_error)?;
        let name = self
            .decoder
            .read_pascal_identifier()
            .map_err(to_syn_error)?;
        if matches!(
            self.decoder.peek_token().map_err(to_syn_error)?,
            Some(Token::LBracket)
        ) {
            let path_ref = self.decoder.read_path().map_err(to_syn_error)?;
            self.decoder.expect_record_end().map_err(to_syn_error)?;
            return Ok(ResolvedDefinition {
                name,
                variants: Vec::new(),
                path_ref: Some(path_ref),
            });
        }

        let mut variants = Vec::new();
        while !self.decoder.peek_is_record_end().map_err(to_syn_error)? {
            variants.push(self.parse_variant()?);
        }
        self.decoder.expect_record_end().map_err(to_syn_error)?;
        Ok(ResolvedDefinition {
            name,
            variants,
            path_ref: None,
        })
    }

    fn parse_variant(&mut self) -> syn::Result<ResolvedVariant> {
        if self.decoder.peek_is_record_start().map_err(to_syn_error)? {
            self.decoder.expect_record_start().map_err(to_syn_error)?;
            let name = self
                .decoder
                .read_pascal_identifier()
                .map_err(to_syn_error)?;
            if matches!(
                self.decoder.peek_token().map_err(to_syn_error)?,
                Some(Token::Ident(ref token)) if token == "belongs"
            ) {
                let marker = self.decoder.read_string().map_err(to_syn_error)?;
                if marker != "belongs" {
                    return Err(syn::Error::new(
                        Span::call_site(),
                        format!("expected belongs marker, got `{marker}`"),
                    ));
                }
                let stream = self
                    .decoder
                    .read_pascal_identifier()
                    .map_err(to_syn_error)?;
                self.decoder.expect_record_end().map_err(to_syn_error)?;
                return Ok(ResolvedVariant::Data {
                    name: name.clone(),
                    fields: vec![ResolvedType::Named(name)],
                    belongs: Some(stream),
                });
            }

            let mut fields = Vec::new();
            while !self.decoder.peek_is_record_end().map_err(to_syn_error)? {
                fields.push(self.parse_type()?);
            }
            self.decoder.expect_record_end().map_err(to_syn_error)?;
            Ok(ResolvedVariant::Data {
                name,
                fields,
                belongs: None,
            })
        } else {
            let name = self
                .decoder
                .read_pascal_identifier()
                .map_err(to_syn_error)?;
            Ok(ResolvedVariant::Unit { name })
        }
    }

    fn parse_type(&mut self) -> syn::Result<ResolvedType> {
        if matches!(
            self.decoder.peek_token().map_err(to_syn_error)?,
            Some(Token::LBracket)
        ) {
            self.decoder.expect_seq_start().map_err(to_syn_error)?;
            let container = self
                .decoder
                .read_pascal_identifier()
                .map_err(to_syn_error)?;
            let inner = self.parse_type()?;
            self.decoder.expect_seq_end().map_err(to_syn_error)?;
            match container.as_str() {
                // DESIGN-DECISION-REVIEW (designer/320 §2.4): minimal
                // primitive/container set. Alternative: include signed
                // ints, floats, char, maps, and custom containers.
                // Revisit if a concrete consumer needs them.
                "Vec" => Ok(ResolvedType::Vec(Box::new(inner))),
                "Option" => Ok(ResolvedType::Option(Box::new(inner))),
                _ => Err(syn::Error::new(
                    Span::call_site(),
                    format!("unknown schema container `{container}`"),
                )),
            }
        } else if let Some(Token::Ident(name)) = self.decoder.peek_token().map_err(to_syn_error)? {
            if name.chars().next().is_some_and(char::is_uppercase) {
                Ok(ResolvedType::Named(
                    self.decoder
                        .read_pascal_identifier()
                        .map_err(to_syn_error)?,
                ))
            } else {
                Ok(ResolvedType::Named(
                    self.decoder.read_string().map_err(to_syn_error)?,
                ))
            }
        } else {
            Err(syn::Error::new(
                Span::call_site(),
                "expected schema type reference",
            ))
        }
    }
}

#[derive(Clone)]
struct ResolvedSchema {
    definitions: BTreeMap<String, ResolvedDefinition>,
}

impl ResolvedSchema {
    fn into_channel_spec(self) -> syn::Result<ChannelSpec> {
        let name = channel_name_from_manifest();
        let request = self.request_block()?;
        let reply = self.reply_block()?;
        let event = self.event_block()?;
        let streams = self.stream_blocks(&request, event.as_ref())?;
        let request = self.request_with_open_streams(request, &streams);
        let observable = observable_from_manifest();
        let schema = Some(SchemaSpec {
            definitions: self
                .definitions
                .iter()
                .map(|(name, definition)| {
                    (
                        name.clone(),
                        SchemaDefinition {
                            name: definition.name.clone(),
                            variants: definition
                                .variants
                                .iter()
                                .map(SchemaVariant::from)
                                .collect(),
                        },
                    )
                })
                .collect(),
        });

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

    fn definition(&self, name: &str) -> syn::Result<&ResolvedDefinition> {
        self.definitions.get(name).ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                format!("schema is missing required root enum `{name}`"),
            )
        })
    }

    fn request_block(&self) -> syn::Result<RequestBlockSpec> {
        let operation = self.definition("Operation")?;
        let variants = operation
            .variants
            .iter()
            .map(|variant| match variant {
                ResolvedVariant::Data { name, fields, .. } if fields.len() == 1 => {
                    Ok(RequestVariantSpec {
                        variant_name: ident(name),
                        payload_type: type_from_ref(&fields[0])?,
                        opens: None,
                    })
                }
                _ => Err(syn::Error::new(
                    Span::call_site(),
                    "Operation variants must be data-carrying variants with one payload type",
                )),
            })
            .collect::<syn::Result<Vec<_>>>()?;
        Ok(RequestBlockSpec {
            name: ident("Operation"),
            variants,
        })
    }

    fn reply_block(&self) -> syn::Result<ReplyBlockSpec> {
        let reply = self.definition("Reply")?;
        let variants = reply
            .variants
            .iter()
            .map(|variant| match variant {
                ResolvedVariant::Data { name, fields, .. } if fields.len() == 1 => {
                    Ok(ReplyVariantSpec {
                        variant_name: ident(name),
                        payload_type: type_from_ref(&fields[0])?,
                    })
                }
                _ => Err(syn::Error::new(
                    Span::call_site(),
                    "Reply variants must be data-carrying variants with one payload type",
                )),
            })
            .collect::<syn::Result<Vec<_>>>()?;
        Ok(ReplyBlockSpec {
            name: ident("Reply"),
            variants,
        })
    }

    fn event_block(&self) -> syn::Result<Option<EventBlockSpec>> {
        let Some(event) = self.definitions.get("Event") else {
            return Ok(None);
        };
        let variants = event
            .variants
            .iter()
            .map(|variant| match variant {
                ResolvedVariant::Data {
                    name,
                    fields,
                    belongs,
                } if fields.len() == 1 => Ok(EventVariantSpec {
                    variant_name: ident(name),
                    payload_type: type_from_ref(&fields[0])?,
                    belongs: belongs.as_ref().map(|name| ident(name)),
                }),
                _ => Err(syn::Error::new(
                    Span::call_site(),
                    "Event variants must be data-carrying variants with one payload type",
                )),
            })
            .collect::<syn::Result<Vec<_>>>()?;
        Ok(Some(EventBlockSpec {
            name: ident("Event"),
            variants,
        }))
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
                "MVP schema reader supports one domain stream per channel",
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

    fn request_with_open_streams(
        &self,
        mut request: RequestBlockSpec,
        streams: &[StreamBlockSpec],
    ) -> RequestBlockSpec {
        if streams.len() == 1 {
            let stream = streams[0].name.clone();
            for variant in &mut request.variants {
                if variant.variant_name == "Watch" {
                    // DESIGN-DECISION-REVIEW (designer/320 §2.5):
                    // belongs annotation on event variants drives MVP
                    // stream synthesis. Alternative: separate stream block
                    // in root vector. Revisit if stream relations grow
                    // multi-event complexity not expressible inline.
                    variant.opens = Some(stream.clone());
                }
            }
        }
        request
    }
}

#[derive(Clone)]
struct ResolvedDefinition {
    name: String,
    variants: Vec<ResolvedVariant>,
    path_ref: Option<String>,
}

#[derive(Clone)]
enum ResolvedVariant {
    Unit {
        name: String,
    },
    Data {
        name: String,
        fields: Vec<ResolvedType>,
        belongs: Option<String>,
    },
}

#[derive(Clone)]
enum ResolvedType {
    Named(String),
    Vec(Box<ResolvedType>),
    Option(Box<ResolvedType>),
}

impl From<&ResolvedVariant> for SchemaVariant {
    fn from(variant: &ResolvedVariant) -> Self {
        match variant {
            ResolvedVariant::Unit { name } => Self::Unit { name: name.clone() },
            ResolvedVariant::Data {
                name,
                fields,
                belongs: _,
            } => Self::Data {
                name: name.clone(),
                fields: fields.iter().map(SchemaType::from).collect(),
            },
        }
    }
}

impl From<&ResolvedType> for SchemaType {
    fn from(value: &ResolvedType) -> Self {
        match value {
            ResolvedType::Named(name) => Self::Named(name.clone()),
            ResolvedType::Vec(inner) => Self::Vec(Box::new(SchemaType::from(inner.as_ref()))),
            ResolvedType::Option(inner) => Self::Option(Box::new(SchemaType::from(inner.as_ref()))),
        }
    }
}

fn type_from_ref(value: &ResolvedType) -> syn::Result<Type> {
    match value {
        ResolvedType::Named(name) => syn::parse_str(name).map_err(|error| {
            syn::Error::new(
                Span::call_site(),
                format!("invalid Rust type `{name}` from schema: {error}"),
            )
        }),
        ResolvedType::Vec(inner) => {
            let inner = type_from_ref(inner)?;
            Ok(parse_quote!(Vec<#inner>))
        }
        ResolvedType::Option(inner) => {
            let inner = type_from_ref(inner)?;
            Ok(parse_quote!(Option<#inner>))
        }
    }
}

fn channel_name_from_manifest() -> Ident {
    let package = std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "Signal".to_string());
    let stem = package
        .strip_prefix("owner-signal-")
        .or_else(|| package.strip_prefix("signal-"))
        .unwrap_or(&package);
    ident(&to_pascal_case(stem))
}

fn observable_from_manifest() -> Option<ObservableBlockSpec> {
    let package = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
    if package == "signal-persona-spirit" {
        Some(ObservableBlockSpec {
            filter: FilterDecl::Default,
            operation_event: ident("OperationReceived"),
            effect_event: ident("EffectEmitted"),
        })
    } else {
        None
    }
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

fn to_syn_error(error: nota_codec::Error) -> syn::Error {
    syn::Error::new(Span::call_site(), error.to_string())
}
