//! NOTA schema reader for `signal_channel!([schema])`.

use std::collections::{BTreeMap, BTreeSet};
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
            format!("cannot read CARGO_MANIFEST_DIR for schema file: {error}"),
        )
    })?;
    let root = PathBuf::from(manifest);
    let path = default_schema_path(&root);
    let schema = SchemaReader::new(root).read_schema(&path)?;
    schema.into_channel_spec()
}

fn default_schema_path(root: &Path) -> PathBuf {
    let package = std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "signal".to_string());
    let stem = schema_file_stem_from_package(&package);
    let schema_path = root.join(format!("{stem}.schema"));
    if schema_path.exists() {
        return schema_path;
    }

    // Compatibility path for the first schema-MVP landing. New
    // contract schemas use `<component>.schema`.
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
                    format!("schema path-ref `{path_ref}` did not define requested type `{name}`"),
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
        let schema = match self.decoder.peek_token().map_err(to_syn_error)? {
            Some(Token::LBracket) => self.parse_vector_schema(),
            Some(Token::LParen) => self.parse_struct_schema(),
            Some(token) => Err(syn::Error::new(
                Span::call_site(),
                format!("expected schema record or legacy definition vector, got `{token:?}`"),
            )),
            None => Err(syn::Error::new(Span::call_site(), "schema is empty")),
        }?;

        if let Some(token) = self.decoder.peek_token().map_err(to_syn_error)? {
            return Err(syn::Error::new(
                Span::call_site(),
                format!("unexpected trailing schema token `{token:?}`"),
            ));
        }
        Ok(schema)
    }

    fn parse_vector_schema(&mut self) -> syn::Result<ResolvedSchema> {
        self.decoder.expect_seq_start().map_err(to_syn_error)?;
        if self.decoder.peek_is_seq_end().map_err(to_syn_error)? {
            self.decoder.expect_seq_end().map_err(to_syn_error)?;
            return self.parse_schema_after_operation_header(Vec::new());
        }

        if self.decoder.peek_is_record_start().map_err(to_syn_error)? {
            self.decoder.expect_record_start().map_err(to_syn_error)?;
            let name = self
                .decoder
                .read_pascal_identifier()
                .map_err(to_syn_error)?;
            if is_channel_section_name(&name) {
                return self.parse_legacy_vector_schema_after_first_section(name);
            }

            let first_variant = self.parse_variant_after_name(name)?;
            let variants = self.parse_variant_vector_after_first(first_variant)?;
            return self.parse_schema_after_operation_header(variants);
        }

        let first_variant = self.parse_variant()?;
        let variants = self.parse_variant_vector_after_first(first_variant)?;
        self.parse_schema_after_operation_header(variants)
    }

    fn parse_legacy_vector_schema_after_first_section(
        &mut self,
        first_name: String,
    ) -> syn::Result<ResolvedSchema> {
        let mut definitions = BTreeMap::new();
        self.parse_channel_section_item(first_name, &mut definitions)?;
        while !self.decoder.peek_is_seq_end().map_err(to_syn_error)? {
            self.decoder.expect_record_start().map_err(to_syn_error)?;
            let name = self
                .decoder
                .read_pascal_identifier()
                .map_err(to_syn_error)?;
            self.parse_channel_section_item(name, &mut definitions)?;
        }
        self.decoder.expect_seq_end().map_err(to_syn_error)?;
        if matches!(
            self.decoder.peek_token().map_err(to_syn_error)?,
            Some(Token::LBrace)
        ) {
            for (name, definition) in self.parse_namespace_map()? {
                definitions.insert(name, definition);
            }
        }
        Ok(ResolvedSchema { definitions })
    }

    fn parse_schema_after_operation_header(
        &mut self,
        operation_variants: Vec<ResolvedVariant>,
    ) -> syn::Result<ResolvedSchema> {
        let mut definitions = BTreeMap::from([(
            "Operation".to_string(),
            ResolvedDefinition {
                name: "Operation".to_string(),
                variants: operation_variants,
                path_ref: None,
                alias: None,
            },
        )]);

        match self.decoder.peek_token().map_err(to_syn_error)? {
            Some(Token::LBracket) => {
                self.parse_unused_header_vector()?;
                self.parse_unused_header_vector()?;

                for (name, definition) in self.parse_namespace_map()? {
                    definitions.insert(name, definition);
                }
                let known_names = definitions.keys().cloned().collect::<BTreeSet<_>>();
                for (name, definition) in self.parse_namespace_map_with_known(&known_names)? {
                    definitions.insert(name, definition);
                }

                if matches!(
                    self.decoder.peek_token().map_err(to_syn_error)?,
                    Some(Token::LBracket)
                ) {
                    for (name, definition) in self.parse_feature_vector()? {
                        definitions.insert(name, definition);
                    }
                }
                Ok(ResolvedSchema { definitions })
            }
            Some(Token::LBrace) => {
                for (name, definition) in self.parse_namespace_map()? {
                    definitions.insert(name, definition);
                }
                Ok(ResolvedSchema { definitions })
            }
            _ => Ok(ResolvedSchema { definitions }),
        }
    }

    fn parse_variant_vector_after_first(
        &mut self,
        first: ResolvedVariant,
    ) -> syn::Result<Vec<ResolvedVariant>> {
        let mut variants = vec![first];
        while !self.decoder.peek_is_seq_end().map_err(to_syn_error)? {
            variants.push(self.parse_variant()?);
        }
        self.decoder.expect_seq_end().map_err(to_syn_error)?;
        Ok(variants)
    }

    fn parse_unused_header_vector(&mut self) -> syn::Result<()> {
        self.decoder.expect_seq_start().map_err(to_syn_error)?;
        while !self.decoder.peek_is_seq_end().map_err(to_syn_error)? {
            let _ = self.parse_variant()?;
        }
        self.decoder.expect_seq_end().map_err(to_syn_error)?;
        Ok(())
    }

    fn parse_feature_vector(&mut self) -> syn::Result<BTreeMap<String, ResolvedDefinition>> {
        self.decoder.expect_seq_start().map_err(to_syn_error)?;
        let mut definitions = BTreeMap::new();
        while !self.decoder.peek_is_seq_end().map_err(to_syn_error)? {
            self.decoder.expect_record_start().map_err(to_syn_error)?;
            let name = self
                .decoder
                .read_pascal_identifier()
                .map_err(to_syn_error)?;
            match name.as_str() {
                "Reply" => {
                    let definition = self.parse_reply_feature_body()?;
                    definitions.insert(definition.name.clone(), definition);
                }
                "Event" => {
                    let definition = self.parse_event_feature_body()?;
                    definitions.insert(definition.name.clone(), definition);
                }
                "Observable" | "Storage" => self.skip_record_body()?,
                other => {
                    return Err(syn::Error::new(
                        Span::call_site(),
                        format!("unknown schema feature `{other}`"),
                    ));
                }
            }
        }
        self.decoder.expect_seq_end().map_err(to_syn_error)?;
        Ok(definitions)
    }

    fn parse_channel_section_item(
        &mut self,
        name: String,
        definitions: &mut BTreeMap<String, ResolvedDefinition>,
    ) -> syn::Result<()> {
        match name.as_str() {
            "Operation" | "Reply" | "Event" => {
                let definition = self.parse_definition_body(name)?;
                definitions.insert(definition.name.clone(), definition);
            }
            "Observable" => self.skip_record_body()?,
            _ => {
                let definition = self.parse_definition_body(name)?;
                definitions.insert(definition.name.clone(), definition);
            }
        }
        Ok(())
    }

    fn parse_struct_schema(&mut self) -> syn::Result<ResolvedSchema> {
        self.decoder.expect_record_start().map_err(to_syn_error)?;
        let mut definitions = match self.decoder.peek_token().map_err(to_syn_error)? {
            Some(Token::Ident(ref head)) if head == "Schema" => {
                let _ = self
                    .decoder
                    .read_pascal_identifier()
                    .map_err(to_syn_error)?;
                self.parse_channel_section()?
            }
            Some(Token::LParen) => self.parse_channel_section()?,
            Some(token) => {
                return Err(syn::Error::new(
                    Span::call_site(),
                    format!(
                        "expected positional schema channel block or legacy `Schema` tag, got `{token:?}`"
                    ),
                ));
            }
            None => {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "unexpected end of schema root",
                ));
            }
        };
        for (name, definition) in self.parse_namespace_section()? {
            definitions.insert(name, definition);
        }
        self.decoder.expect_record_end().map_err(to_syn_error)?;
        Ok(ResolvedSchema { definitions })
    }

    fn parse_channel_section(&mut self) -> syn::Result<BTreeMap<String, ResolvedDefinition>> {
        self.decoder.expect_record_start().map_err(to_syn_error)?;
        if matches!(
            self.decoder.peek_token().map_err(to_syn_error)?,
            Some(Token::Ident(ref head)) if head == "Channel"
        ) {
            let _ = self
                .decoder
                .read_pascal_identifier()
                .map_err(to_syn_error)?;
        }

        let mut definitions = BTreeMap::new();
        while !self.decoder.peek_is_record_end().map_err(to_syn_error)? {
            self.decoder.expect_record_start().map_err(to_syn_error)?;
            let name = self
                .decoder
                .read_pascal_identifier()
                .map_err(to_syn_error)?;
            match name.as_str() {
                "Operation" | "Reply" | "Event" => {
                    let definition = self.parse_definition_body(name)?;
                    definitions.insert(definition.name.clone(), definition);
                }
                "Observable" => self.skip_record_body()?,
                other => {
                    return Err(syn::Error::new(
                        Span::call_site(),
                        format!("unknown Channel section `{other}`"),
                    ));
                }
            }
        }
        self.decoder.expect_record_end().map_err(to_syn_error)?;
        Ok(definitions)
    }

    fn parse_namespace_section(&mut self) -> syn::Result<BTreeMap<String, ResolvedDefinition>> {
        if matches!(
            self.decoder.peek_token().map_err(to_syn_error)?,
            Some(Token::LBrace)
        ) {
            return self.parse_namespace_map();
        }

        self.decoder.expect_record_start().map_err(to_syn_error)?;
        let head = self
            .decoder
            .read_pascal_identifier()
            .map_err(to_syn_error)?;
        if head != "Namespace" {
            return Err(syn::Error::new(
                Span::call_site(),
                format!("expected second schema section `Namespace`, got `{head}`"),
            ));
        }
        let definitions = self.parse_namespace_map()?;
        self.decoder.expect_record_end().map_err(to_syn_error)?;
        Ok(definitions)
    }

    fn parse_namespace_map(&mut self) -> syn::Result<BTreeMap<String, ResolvedDefinition>> {
        self.parse_namespace_map_with_known(&BTreeSet::new())
    }

    fn parse_namespace_map_with_known(
        &mut self,
        extra_known_names: &BTreeSet<String>,
    ) -> syn::Result<BTreeMap<String, ResolvedDefinition>> {
        self.decoder.expect_map_start().map_err(to_syn_error)?;
        let mut raw_definitions = BTreeMap::new();
        while !self.decoder.peek_is_map_end().map_err(to_syn_error)? {
            let name = self.decoder.read_map_key().map_err(to_syn_error)?;
            validate_schema_name(&name)?;
            let definition = self.parse_namespace_definition(name.clone())?;
            raw_definitions.insert(name, definition);
        }
        self.decoder.expect_map_end().map_err(to_syn_error)?;

        let mut known_names = extra_known_names.clone();
        known_names.extend(raw_definitions.keys().cloned());
        raw_definitions
            .into_iter()
            .map(|(name, definition)| {
                definition
                    .into_resolved(&known_names)
                    .map(|definition| (name, definition))
            })
            .collect()
    }

    fn parse_namespace_definition(&mut self, name: String) -> syn::Result<RawNamespaceDefinition> {
        if matches!(
            self.decoder.peek_token().map_err(to_syn_error)?,
            Some(Token::LBracket)
        ) {
            let variants = self.parse_namespace_enum_items()?;
            return Ok(RawNamespaceDefinition {
                name,
                items: variants
                    .into_iter()
                    .map(RawNamespaceItem::Variant)
                    .collect(),
                path_ref: None,
                alias: None,
            });
        }

        if matches!(
            self.decoder.peek_token().map_err(to_syn_error)?,
            Some(Token::Ident(_))
        ) {
            let alias = self.parse_type()?;
            return Ok(RawNamespaceDefinition {
                name,
                items: Vec::new(),
                path_ref: None,
                alias: Some(alias),
            });
        }

        if !self.decoder.peek_is_record_start().map_err(to_syn_error)? {
            return Err(syn::Error::new(
                Span::call_site(),
                format!(
                    "schema namespace entry `{name}` must carry an enum vector, struct record, path, or alias"
                ),
            ));
        }

        self.decoder.expect_record_start().map_err(to_syn_error)?;
        if matches!(
            self.decoder.peek_token().map_err(to_syn_error)?,
            Some(Token::Ident(ref token)) if token == "Path"
        ) {
            let _ = self
                .decoder
                .read_pascal_identifier()
                .map_err(to_syn_error)?;
            let path_ref = self.decoder.read_path().map_err(to_syn_error)?;
            self.decoder.expect_record_end().map_err(to_syn_error)?;
            return Ok(RawNamespaceDefinition {
                name,
                items: Vec::new(),
                path_ref: Some(path_ref),
                alias: None,
            });
        }
        let first_type = self.parse_type()?;
        if matches!(&first_type, ResolvedType::Named(type_name) if type_name == &name) {
            return Err(syn::Error::new(
                Span::call_site(),
                format!(
                    "schema namespace entry `{name}` must not restate its key inside the value"
                ),
            ));
        }

        let mut items = vec![RawNamespaceItem::Type(first_type)];
        while !self.decoder.peek_is_record_end().map_err(to_syn_error)? {
            if self.decoder.peek_is_record_start().map_err(to_syn_error)? {
                return Err(syn::Error::new(
                    Span::call_site(),
                    format!(
                        "schema struct declaration `{name}` cannot contain data-carrying enum variants"
                    ),
                ));
            } else {
                items.push(RawNamespaceItem::Type(self.parse_type()?));
            }
        }
        self.decoder.expect_record_end().map_err(to_syn_error)?;

        Ok(RawNamespaceDefinition {
            name,
            items,
            path_ref: None,
            alias: None,
        })
    }

    fn parse_namespace_enum_items(&mut self) -> syn::Result<Vec<ResolvedVariant>> {
        self.decoder.expect_seq_start().map_err(to_syn_error)?;
        let mut variants = Vec::new();
        while !self.decoder.peek_is_seq_end().map_err(to_syn_error)? {
            variants.push(self.parse_variant()?);
        }
        self.decoder.expect_seq_end().map_err(to_syn_error)?;
        Ok(variants)
    }

    fn parse_definition_body(&mut self, name: String) -> syn::Result<ResolvedDefinition> {
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
                alias: None,
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
            alias: None,
        })
    }

    fn parse_reply_feature_body(&mut self) -> syn::Result<ResolvedDefinition> {
        let mut variants = Vec::new();
        while !self.decoder.peek_is_record_end().map_err(to_syn_error)? {
            variants.push(self.parse_bare_payload_variant(None)?);
        }
        self.decoder.expect_record_end().map_err(to_syn_error)?;
        Ok(ResolvedDefinition {
            name: "Reply".to_string(),
            variants,
            path_ref: None,
            alias: None,
        })
    }

    fn parse_event_feature_body(&mut self) -> syn::Result<ResolvedDefinition> {
        let mut belongs = None;
        if self.decoder.peek_is_record_start().map_err(to_syn_error)? {
            self.decoder.expect_record_start().map_err(to_syn_error)?;
            let head = self.decoder.read_string().map_err(to_syn_error)?;
            if head == "belongs" {
                belongs = Some(
                    self.decoder
                        .read_pascal_identifier()
                        .map_err(to_syn_error)?,
                );
                self.decoder.expect_record_end().map_err(to_syn_error)?;
            } else {
                return Err(syn::Error::new(
                    Span::call_site(),
                    format!("expected Event annotation `(belongs Stream)`, got `({head} ...)`"),
                ));
            }
        }

        let mut variants = Vec::new();
        while !self.decoder.peek_is_record_end().map_err(to_syn_error)? {
            variants.push(self.parse_bare_payload_variant(belongs.clone())?);
        }
        self.decoder.expect_record_end().map_err(to_syn_error)?;
        Ok(ResolvedDefinition {
            name: "Event".to_string(),
            variants,
            path_ref: None,
            alias: None,
        })
    }

    fn parse_bare_payload_variant(
        &mut self,
        block_belongs: Option<String>,
    ) -> syn::Result<ResolvedVariant> {
        if self.decoder.peek_is_record_start().map_err(to_syn_error)? {
            let mut variant = self.parse_variant()?;
            if let (
                Some(stream),
                ResolvedVariant::Data {
                    belongs: variant_belongs,
                    ..
                },
            ) = (&block_belongs, &mut variant)
            {
                if variant_belongs.is_none() {
                    *variant_belongs = Some(stream.clone());
                }
            }
            return Ok(variant);
        }

        let name = self
            .decoder
            .read_pascal_identifier()
            .map_err(to_syn_error)?;
        Ok(ResolvedVariant::Data {
            name: name.clone(),
            fields: vec![ResolvedType::Named(name)],
            belongs: block_belongs,
        })
    }

    fn skip_record_body(&mut self) -> syn::Result<()> {
        while !self.decoder.peek_is_record_end().map_err(to_syn_error)? {
            self.skip_value()?;
        }
        self.decoder.expect_record_end().map_err(to_syn_error)?;
        Ok(())
    }

    fn skip_value(&mut self) -> syn::Result<()> {
        match self.decoder.peek_token().map_err(to_syn_error)? {
            Some(Token::LParen) => {
                self.decoder.expect_record_start().map_err(to_syn_error)?;
                while !self.decoder.peek_is_record_end().map_err(to_syn_error)? {
                    self.skip_value()?;
                }
                self.decoder.expect_record_end().map_err(to_syn_error)
            }
            Some(Token::LBracket) => {
                let _ = self.decoder.read_path().map_err(to_syn_error)?;
                Ok(())
            }
            Some(Token::LBrace) => {
                self.decoder.expect_map_start().map_err(to_syn_error)?;
                while !self.decoder.peek_is_map_end().map_err(to_syn_error)? {
                    let _ = self.decoder.read_map_key().map_err(to_syn_error)?;
                    self.skip_value()?;
                }
                self.decoder.expect_map_end().map_err(to_syn_error)
            }
            Some(Token::Ident(name)) if name.chars().next().is_some_and(char::is_uppercase) => {
                let _ = self
                    .decoder
                    .read_pascal_identifier()
                    .map_err(to_syn_error)?;
                Ok(())
            }
            Some(Token::Ident(_)) | Some(Token::Str(_)) => {
                let _ = self.decoder.read_string().map_err(to_syn_error)?;
                Ok(())
            }
            Some(token) => Err(syn::Error::new(
                Span::call_site(),
                format!("cannot skip unsupported schema token `{token:?}`"),
            )),
            None => Err(syn::Error::new(
                Span::call_site(),
                "unexpected end of schema while skipping value",
            )),
        }
    }

    fn parse_variant(&mut self) -> syn::Result<ResolvedVariant> {
        if self.decoder.peek_is_record_start().map_err(to_syn_error)? {
            self.decoder.expect_record_start().map_err(to_syn_error)?;
            let name = self
                .decoder
                .read_pascal_identifier()
                .map_err(to_syn_error)?;
            self.parse_variant_after_name(name)
        } else {
            let name = self
                .decoder
                .read_pascal_identifier()
                .map_err(to_syn_error)?;
            Ok(ResolvedVariant::Unit { name })
        }
    }

    fn parse_variant_after_name(&mut self, name: String) -> syn::Result<ResolvedVariant> {
        if matches!(
            self.decoder.peek_token().map_err(to_syn_error)?,
            Some(Token::LBracket)
        ) {
            let payload = self.parse_bracket_variant_payload(&name)?;
            self.decoder.expect_record_end().map_err(to_syn_error)?;
            return Ok(ResolvedVariant::Data {
                name,
                fields: vec![payload],
                belongs: None,
            });
        }

        if self.decoder.peek_is_record_start().map_err(to_syn_error)? {
            self.decoder.expect_record_start().map_err(to_syn_error)?;
            let field_name = self
                .decoder
                .read_pascal_identifier()
                .map_err(to_syn_error)?;
            if self.decoder.peek_is_record_start().map_err(to_syn_error)? {
                self.parse_engine_annotation()?;
                self.decoder.expect_record_end().map_err(to_syn_error)?;
                self.decoder.expect_record_end().map_err(to_syn_error)?;
                return Ok(ResolvedVariant::Data {
                    name,
                    fields: vec![ResolvedType::Named(field_name)],
                    belongs: None,
                });
            }
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
                    name,
                    fields: vec![ResolvedType::Named(field_name)],
                    belongs: Some(stream),
                });
            }
            self.decoder.expect_record_end().map_err(to_syn_error)?;
            self.decoder.expect_record_end().map_err(to_syn_error)?;
            return Ok(ResolvedVariant::Data {
                name,
                fields: vec![ResolvedType::Named(field_name)],
                belongs: None,
            });
        }
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
    }

    fn parse_bracket_variant_payload(
        &mut self,
        variant_name: &str,
    ) -> syn::Result<ResolvedType> {
        self.decoder.expect_seq_start().map_err(to_syn_error)?;
        if self.decoder.peek_is_seq_end().map_err(to_syn_error)? {
            return Err(syn::Error::new(
                Span::call_site(),
                format!("schema header node `{variant_name}` must name one endpoint"),
            ));
        }

        let endpoint = self
            .decoder
            .read_pascal_identifier()
            .map_err(to_syn_error)?;
        if endpoint == "Vec" || endpoint == "Option" {
            let inner = self.parse_type()?;
            self.decoder.expect_seq_end().map_err(to_syn_error)?;
            return match endpoint.as_str() {
                "Vec" => Ok(ResolvedType::Vec(Box::new(inner))),
                "Option" => Ok(ResolvedType::Option(Box::new(inner))),
                _ => unreachable!(),
            };
        }

        if !self.decoder.peek_is_seq_end().map_err(to_syn_error)? {
            return Err(syn::Error::new(
                Span::call_site(),
                format!(
                    "schema header node `{variant_name}` must name exactly one endpoint in the MVP"
                ),
            ));
        }
        self.decoder.expect_seq_end().map_err(to_syn_error)?;

        Ok(ResolvedType::Named(endpoint))
    }

    fn parse_engine_annotation(&mut self) -> syn::Result<()> {
        self.decoder.expect_record_start().map_err(to_syn_error)?;
        let head = self.decoder.read_string().map_err(to_syn_error)?;
        if head != "engine" {
            return Err(syn::Error::new(
                Span::call_site(),
                format!("expected schema annotation `(engine <class>)`, got `{head}`"),
            ));
        }
        let _class = self.decoder.read_string().map_err(to_syn_error)?;
        self.decoder.expect_record_end().map_err(to_syn_error)?;
        Ok(())
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
                            alias: definition.alias.as_ref().map(SchemaType::from),
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
    alias: Option<ResolvedType>,
}

struct RawNamespaceDefinition {
    name: String,
    items: Vec<RawNamespaceItem>,
    path_ref: Option<String>,
    alias: Option<ResolvedType>,
}

impl RawNamespaceDefinition {
    fn into_resolved(self, known_names: &BTreeSet<String>) -> syn::Result<ResolvedDefinition> {
        if let Some(alias) = self.alias {
            if !schema_type_resolves(&alias, known_names) {
                return Err(syn::Error::new(
                    Span::call_site(),
                    format!(
                        "schema alias `{}` points at unknown type `{}`",
                        self.name,
                        schema_type_name(&alias)
                    ),
                ));
            }
            return Ok(ResolvedDefinition {
                name: self.name,
                variants: Vec::new(),
                path_ref: None,
                alias: Some(alias),
            });
        }

        let Some(path_ref) = self.path_ref else {
            let variants = resolve_namespace_items(&self.name, self.items, known_names)?;
            return Ok(ResolvedDefinition {
                name: self.name,
                variants,
                path_ref: None,
                alias: None,
            });
        };

        Ok(ResolvedDefinition {
            name: self.name,
            variants: Vec::new(),
            path_ref: Some(path_ref),
            alias: None,
        })
    }
}

enum RawNamespaceItem {
    Type(ResolvedType),
    Variant(ResolvedVariant),
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

fn resolve_namespace_items(
    name: &str,
    items: Vec<RawNamespaceItem>,
    known_names: &BTreeSet<String>,
) -> syn::Result<Vec<ResolvedVariant>> {
    let has_explicit_variant = items
        .iter()
        .any(|item| matches!(item, RawNamespaceItem::Variant(_)));
    if has_explicit_variant {
        return items
            .into_iter()
            .map(|item| match item {
                RawNamespaceItem::Variant(variant) => Ok(variant),
                RawNamespaceItem::Type(ResolvedType::Named(name)) => {
                    Ok(ResolvedVariant::Unit { name })
                }
                RawNamespaceItem::Type(_) => Err(syn::Error::new(
                    Span::call_site(),
                    "mixed enum namespace declarations can only use bare unit variants or nested data variants",
                )),
            })
            .collect();
    }

    let types = items
        .into_iter()
        .map(|item| match item {
            RawNamespaceItem::Type(schema_type) => Ok(schema_type),
            RawNamespaceItem::Variant(_) => unreachable!("checked explicit variants above"),
        })
        .collect::<syn::Result<Vec<_>>>()?;

    if types
        .iter()
        .all(|schema_type| schema_type_resolves(schema_type, known_names))
    {
        return Ok(vec![ResolvedVariant::Data {
            name: name.to_string(),
            fields: types,
            belongs: None,
        }]);
    }

    types
        .into_iter()
        .map(|schema_type| match schema_type {
            ResolvedType::Named(name) => Ok(ResolvedVariant::Unit { name }),
            ResolvedType::Vec(_) | ResolvedType::Option(_) => Err(syn::Error::new(
                Span::call_site(),
                "unit enum namespace declarations cannot use container type syntax",
            )),
        })
        .collect()
}

fn schema_type_resolves(schema_type: &ResolvedType, known_names: &BTreeSet<String>) -> bool {
    match schema_type {
        ResolvedType::Named(name) => is_schema_primitive(name) || known_names.contains(name),
        ResolvedType::Vec(inner) | ResolvedType::Option(inner) => {
            schema_type_resolves(inner, known_names)
        }
    }
}

fn schema_type_name(schema_type: &ResolvedType) -> String {
    match schema_type {
        ResolvedType::Named(name) => name.clone(),
        ResolvedType::Vec(inner) => format!("Vec<{}>", schema_type_name(inner)),
        ResolvedType::Option(inner) => format!("Option<{}>", schema_type_name(inner)),
    }
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

fn validate_schema_name(name: &str) -> syn::Result<()> {
    if name.chars().next().is_some_and(char::is_uppercase) {
        Ok(())
    } else {
        Err(syn::Error::new(
            Span::call_site(),
            format!("schema namespace key `{name}` must be PascalCase"),
        ))
    }
}

fn is_schema_primitive(name: &str) -> bool {
    matches!(
        name,
        "String" | "u8" | "u16" | "u32" | "u64" | "bool" | "Date" | "Time" | "Bytes"
    )
}

fn is_channel_section_name(name: &str) -> bool {
    matches!(name, "Operation" | "Reply" | "Event" | "Observable")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_tagged_schema_record_with_namespace_map() {
        let text = r#"
            (Schema
              (Channel
                (Operation
                  (Record (Entry (engine assert)))
                  (Observe Observation)
                  (Unwatch SubscriptionToken))
                (Reply
                  (RecordAccepted RecordAccepted)))
              (Namespace
                {
                  Entry (Topic)
                  Topic (String)
                  Observation [State]
                  State (String)
                  RecordAccepted (RecordIdentifier)
                  RecordIdentifier (u64)
                  RecordCaptured (RecordAccepted)
                  SubscriptionToken (u64)
                }))
        "#;

        let mut parser = SchemaParser::new(text);
        let schema = parser.parse().expect("schema parses");
        assert!(schema.definitions.contains_key("Operation"));
        assert!(schema.definitions.contains_key("Reply"));
        assert!(schema.definitions.contains_key("Entry"));

        let channel = schema.into_channel_spec().expect("channel spec");
        assert_eq!(channel.request.variants[0].variant_name, "Record");
        assert!(channel.streams.is_empty());
    }

    #[test]
    fn parses_schema_file_body_with_header_vectors_and_namespace_map() {
        let text = r#"
            [
              (Record [Entry])
              (Observe [Observation])
              (Unwatch [SubscriptionToken])
            ]

            []

            []

            {
              Magnitude (Path schemas/signal-sema/magnitude.schema)
            }

            {
              Kind [Decision Principle Correction Clarification Constraint]
              Entry (Topic Kind Magnitude)
              Topic (String)
              WorkspaceTopic Topic
              Observation [State (Records RecordQuery) Topics]
              RecordQuery ([Option Topic] [Option Kind])
              State (String)
              RecordAccepted (RecordIdentifier)
              RecordIdentifier (u64)
              RecordCaptured (RecordAccepted)
              SubscriptionToken (u64)
            }

            [
              (Reply RecordAccepted)
              (Event (belongs DomainStream) RecordCaptured)
              (Observable
                (filter default)
                (operation_event OperationReceived)
                (effect_event EffectEmitted))
            ]
        "#;

        let mut parser = SchemaParser::new(text);
        let schema = parser.parse().expect("schema parses");
        assert!(schema.definitions.contains_key("Operation"));
        assert_eq!(
            schema
                .definitions
                .get("Magnitude")
                .and_then(|definition| definition.path_ref.as_deref()),
            Some("schemas/signal-sema/magnitude.schema")
        );
        assert!(matches!(
            &schema.definitions["Kind"].variants[..],
            [
                ResolvedVariant::Unit { name: decision },
                ResolvedVariant::Unit { name: principle },
                ..
            ] if decision == "Decision" && principle == "Principle"
        ));
        assert!(matches!(
            &schema.definitions["Entry"].variants[..],
            [ResolvedVariant::Data { name, fields, .. }]
                if name == "Entry" && fields.len() == 3
        ));
        assert!(matches!(
            &schema.definitions["WorkspaceTopic"].alias,
            Some(ResolvedType::Named(name)) if name == "Topic"
        ));

        let channel = schema.into_channel_spec().expect("channel spec");
        assert_eq!(channel.request.variants[0].variant_name, "Record");
        assert_eq!(channel.reply.variants[0].variant_name, "RecordAccepted");
        assert_eq!(
            channel.streams[0].events,
            vec![Ident::new("RecordCaptured", Span::call_site())]
        );
    }

    #[test]
    fn schema_file_body_rejects_multi_endpoint_header_node_for_now() {
        let text = r#"
            [
              (Record [Entry ExtraEndpoint])
            ]
            []
            []
            {}
            {
              Entry (String)
              ExtraEndpoint (String)
            }
            []
        "#;

        let mut parser = SchemaParser::new(text);
        let error = match parser.parse() {
            Ok(_) => panic!("multi-endpoint header node accepted"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("schema header node `Record` must name exactly one endpoint")
        );
    }

    #[test]
    fn namespace_map_rejects_repeated_key_inside_struct_value() {
        let text = r#"
            (Schema
              (Channel
                (Operation (Record Entry))
                (Reply (RecordAccepted RecordAccepted)))
              (Namespace
                {
                  Entry (Entry Topic)
                }))
        "#;

        let mut parser = SchemaParser::new(text);
        let error = match parser.parse() {
            Ok(_) => panic!("repeated declaration key accepted"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("schema namespace entry `Entry` must not restate its key")
        );
    }

    #[test]
    fn namespace_map_rejects_alias_to_unknown_type() {
        let text = r#"
            []
            []
            []
            {}
            {
              Entry MissingType
            }
            []
        "#;

        let mut parser = SchemaParser::new(text);
        let error = match parser.parse() {
            Ok(_) => panic!("unknown alias accepted"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("schema alias `Entry` points at unknown type `MissingType`")
        );
    }

    #[test]
    fn schema_file_body_rejects_trailing_tokens() {
        let text = r#"
            []
            []
            []
            {}
            {
              Entry (String)
            }
            []
            Extra
        "#;

        let mut parser = SchemaParser::new(text);
        let error = match parser.parse() {
            Ok(_) => panic!("trailing schema token accepted"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("unexpected trailing schema token")
        );
    }
}
