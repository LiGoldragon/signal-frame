//! Semantic validation. Returns a `syn::Error` whose spans point at
//! the offending input. Compile-time diagnostics before emission.

use std::collections::HashSet;

use syn::{Error, Type};

use crate::model::ChannelSpec;

pub(crate) fn validate(spec: &ChannelSpec) -> syn::Result<()> {
    validate_variant_uniqueness(spec)?;
    validate_record_head_uniqueness(spec)?;
    validate_stream_relations(spec)?;
    Ok(())
}

fn validate_variant_uniqueness(spec: &ChannelSpec) -> syn::Result<()> {
    let mut seen: HashSet<String> = HashSet::new();
    for variant in &spec.request.variants {
        let name = variant.variant_name.to_string();
        if !seen.insert(name.clone()) {
            return Err(Error::new_spanned(
                &variant.variant_name,
                format!("duplicate variant name `{name}` in request block"),
            ));
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    for variant in &spec.reply.variants {
        let name = variant.variant_name.to_string();
        if !seen.insert(name.clone()) {
            return Err(Error::new_spanned(
                &variant.variant_name,
                format!("duplicate variant name `{name}` in reply block"),
            ));
        }
    }
    if let Some(event) = &spec.event {
        let mut seen: HashSet<String> = HashSet::new();
        for variant in &event.variants {
            let name = variant.variant_name.to_string();
            if !seen.insert(name.clone()) {
                return Err(Error::new_spanned(
                    &variant.variant_name,
                    format!("duplicate variant name `{name}` in event block"),
                ));
            }
        }
    }
    Ok(())
}

fn validate_record_head_uniqueness(spec: &ChannelSpec) -> syn::Result<()> {
    // The NOTA decoder dispatches by record head, not by Rust type
    // path. `domain_a::Status` and `domain_b::Status` both project to
    // `(Status ...)`, so they collide inside the same payload enum.
    flag_duplicate_record_heads(
        spec.request
            .variants
            .iter()
            .map(|v| (&v.variant_name, &v.payload_type)),
        "request",
    )?;
    flag_duplicate_record_heads(
        spec.reply
            .variants
            .iter()
            .map(|v| (&v.variant_name, &v.payload_type)),
        "reply",
    )?;
    if let Some(event) = &spec.event {
        flag_duplicate_record_heads(
            event
                .variants
                .iter()
                .map(|v| (&v.variant_name, &v.payload_type)),
            "event",
        )?;
    }
    Ok(())
}

fn flag_duplicate_record_heads<'spec>(
    variants: impl Iterator<Item = (&'spec syn::Ident, &'spec Type)>,
    block_kind: &'static str,
) -> syn::Result<()> {
    let mut seen: Vec<(String, &syn::Ident)> = Vec::new();
    for (name, payload) in variants {
        let head = projected_record_head(payload);
        if let Some((_, prior)) = seen.iter().find(|(text, _)| *text == head) {
            return Err(Error::new_spanned(
                name,
                format!(
                    "duplicate NOTA record head `{head}` in {block_kind} block — also used by variant `{}`",
                    prior,
                ),
            ));
        }
        seen.push((head, name));
    }
    Ok(())
}

fn projected_record_head(payload: &Type) -> String {
    let payload_text = quote::quote!(#payload).to_string().replace(' ', "");
    payload_text
        .rsplit("::")
        .next()
        .unwrap_or(&payload_text)
        .to_string()
}

fn validate_stream_relations(spec: &ChannelSpec) -> syn::Result<()> {
    let has_streams = !spec.streams.is_empty();
    let has_events = spec.event.is_some();

    if has_streams && !has_events {
        return Err(Error::new_spanned(
            &spec.name,
            "channel has `stream` blocks but no `event` block — declare the event payloads",
        ));
    }
    if has_events && !has_streams {
        return Err(Error::new_spanned(
            &spec.name,
            "channel has an `event` block but no `stream` block — events must belong to a declared stream",
        ));
    }

    let stream_names: HashSet<String> = spec.streams.iter().map(|s| s.name.to_string()).collect();

    let mut opened_stream_names: HashSet<String> = HashSet::new();
    for variant in &spec.request.variants {
        if let Some(opens) = &variant.opens {
            let opens_name = opens.to_string();
            if !stream_names.contains(&opens_name) {
                return Err(Error::new_spanned(
                    opens,
                    format!("`opens {opens_name}` does not resolve to a declared stream block"),
                ));
            }
            opened_stream_names.insert(opens_name);
        }
    }

    for stream in &spec.streams {
        let stream_name = stream.name.to_string();
        if !opened_stream_names.contains(&stream_name) {
            return Err(Error::new_spanned(
                &stream.name,
                format!(
                    "stream `{stream_name}` is orphaned — no request operation opens it",
                ),
            ));
        }
    }

    if let Some(event) = &spec.event {
        for variant in &event.variants {
            match &variant.belongs {
                Some(belongs) => {
                    let belongs_name = belongs.to_string();
                    if !stream_names.contains(&belongs_name) {
                        return Err(Error::new_spanned(
                            belongs,
                            format!(
                                "`belongs {belongs_name}` does not resolve to a declared stream block"
                            ),
                        ));
                    }
                }
                None => {
                    return Err(Error::new_spanned(
                        &variant.variant_name,
                        "event variant must annotate `belongs <StreamName>`",
                    ));
                }
            }
        }
    }

    // Stream block cross-references: opened/event/close must resolve to
    // variants in the corresponding reply/event/request blocks.
    let reply_variants: HashSet<String> = spec
        .reply
        .variants
        .iter()
        .map(|v| v.variant_name.to_string())
        .collect();
    let request_variants: HashSet<String> = spec
        .request
        .variants
        .iter()
        .map(|v| v.variant_name.to_string())
        .collect();
    let event_variants: HashSet<String> = spec
        .event
        .as_ref()
        .map(|event| {
            event
                .variants
                .iter()
                .map(|v| v.variant_name.to_string())
                .collect()
        })
        .unwrap_or_default();

    for stream in &spec.streams {
        if !reply_variants.contains(&stream.opened.to_string()) {
            return Err(Error::new_spanned(
                &stream.opened,
                format!(
                    "stream `{}`: `opened {}` does not resolve to a variant in the reply block",
                    stream.name, stream.opened,
                ),
            ));
        }
        if !event_variants.contains(&stream.event_variant.to_string()) {
            return Err(Error::new_spanned(
                &stream.event_variant,
                format!(
                    "stream `{}`: `event {}` does not resolve to a variant in the event block",
                    stream.name, stream.event_variant,
                ),
            ));
        }
        let event_variant = spec
            .event
            .as_ref()
            .and_then(|event| {
                event
                    .variants
                    .iter()
                    .find(|variant| variant.variant_name == stream.event_variant)
            })
            .expect("event variant verified above");
        let event_belongs = event_variant
            .belongs
            .as_ref()
            .expect("event belongs relation verified above");
        if event_belongs != &stream.name {
            return Err(Error::new_spanned(
                &stream.event_variant,
                format!(
                    "stream `{}` names event `{}` but that event belongs to stream `{}`",
                    stream.name, stream.event_variant, event_belongs,
                ),
            ));
        }
        if !request_variants.contains(&stream.close.to_string()) {
            return Err(Error::new_spanned(
                &stream.close,
                format!(
                    "stream `{}`: `close {}` does not resolve to a variant in the request block",
                    stream.name, stream.close,
                ),
            ));
        }

        let close_variant = spec
            .request
            .variants
            .iter()
            .find(|v| v.variant_name == stream.close)
            .expect("close variant verified above");
        let token = &stream.token;
        let payload_type = &close_variant.payload_type;
        let stream_token = quote::quote!(#token).to_string();
        let close_payload = quote::quote!(#payload_type).to_string();
        if stream_token != close_payload {
            return Err(Error::new_spanned(
                &stream.close,
                format!(
                    "stream `{}`: close variant carries `{close_payload}` but stream's `token` is `{stream_token}` — types must match",
                    stream.name,
                ),
            ));
        }
    }

    Ok(())
}
