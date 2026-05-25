//! `emit_schema!` proc-macro entrypoint.
//!
//! This module is intentionally separate from the legacy handwritten
//! parser/emitter. Schema input flows through `schema::LoadedSchema`
//! and `schema_rust::RustComposer`.

use std::path::{Path, PathBuf};

use proc_macro::TokenStream;
use proc_macro2::Span;
use syn::parse::{Parse, ParseStream};
use syn::{LitStr, Result};

pub(crate) fn emit_schema(input: TokenStream) -> TokenStream {
    match expand(input) {
        Ok(tokens) => tokens,
        Err(error) => error.into_compile_error().into(),
    }
}

fn expand(input: TokenStream) -> Result<TokenStream> {
    let request = syn::parse::<EmitSchemaRequest>(input)?;
    let path = request.schema_path()?;
    let loaded = schema::LoadedSchema::read_path(&path)
        .map_err(|error| syn::Error::new(Span::call_site(), error.to_string()))?;
    let module = schema_rust::RustComposer::new(&loaded)
        .compose()
        .map_err(|error| syn::Error::new(Span::call_site(), error.to_string()))?;
    Ok(module.into_tokens().into())
}

struct EmitSchemaRequest {
    path: Option<LitStr>,
}

impl EmitSchemaRequest {
    fn schema_path(&self) -> Result<PathBuf> {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").map_err(|error| {
            syn::Error::new(
                Span::call_site(),
                format!("cannot read CARGO_MANIFEST_DIR for schema file: {error}"),
            )
        })?;
        let root = PathBuf::from(manifest);
        Ok(match &self.path {
            Some(path) => root.join(path.value()),
            None => default_schema_path(&root),
        })
    }
}

impl Parse for EmitSchemaRequest {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        if input.is_empty() {
            return Ok(Self { path: None });
        }
        let path = input.parse::<LitStr>()?;
        if !input.is_empty() {
            input.parse::<syn::Token![,]>()?;
        }
        if !input.is_empty() {
            return Err(input.error("emit_schema! accepts at most one schema path"));
        }
        Ok(Self { path: Some(path) })
    }
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
