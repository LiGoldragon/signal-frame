//! Semantic validation. Returns a `syn::Error` whose spans point at
//! the offending input. Compile-time diagnostics before emission.

use std::collections::HashSet;

use syn::{Error, Type};

use crate::model::ChannelSpec;

pub(crate) fn validate(spec: &ChannelSpec) -> syn::Result<()> {
    validate_observable_does_not_collide(spec)?;
    validate_variant_uniqueness(spec)?;
    validate_record_head_uniqueness(spec)?;
    validate_stream_relations(spec)?;
    Ok(())
}

/// The observable block injects the standardized `Tap` / `Untap`
/// operations (macro-mandated per /246 §2), a stream named
/// `ObserverStream`, a reply variant `ObserverSubscriptionOpened`,
/// and event variants for the two event roles. Catch contract
/// collisions early with a pointed diagnostic, before the generic
/// duplicate-variant check fires on the macro-expanded output.
///
/// Domain contracts that legitimately want a verb named `Tap` or
/// `Untap` rename their domain verb — the observability verbs are
/// fixed so `persona-introspect` sees a uniform vocabulary across
/// every observable channel (per psyche affirmation
/// 2026-05-20T02:00Z, recorded in `intent/component-shape.nota`).
fn validate_observable_does_not_collide(spec: &ChannelSpec) -> syn::Result<()> {
    let Some(observable) = &spec.observable else {
        return Ok(());
    };

    let operation_event_name = observable.operation_event.to_string();
    let effect_event_name = observable.effect_event.to_string();

    if operation_event_name == effect_event_name {
        return Err(Error::new_spanned(
            &observable.effect_event,
            "observable `operation_event` and `effect_event` must declare distinct event record types",
        ));
    }
    for reserved in ["ObserverSubscriptionToken", "ObserverSubscriptionOpened"] {
        if operation_event_name == reserved {
            return Err(Error::new_spanned(
                &observable.operation_event,
                format!(
                    "observable `operation_event` `{reserved}` collides with a macro-reserved record head"
                ),
            ));
        }
        if effect_event_name == reserved {
            return Err(Error::new_spanned(
                &observable.effect_event,
                format!(
                    "observable `effect_event` `{reserved}` collides with a macro-reserved record head"
                ),
            ));
        }
    }

    for variant in &spec.request.variants {
        let name = variant.variant_name.to_string();
        if name == "Tap" {
            return Err(Error::new_spanned(
                &variant.variant_name,
                "operation `Tap` collides with the macro-mandated observable open verb; rename the domain operation (the observability verbs `Tap`/`Untap` are fixed per /246 §2 for persona-introspect vocabulary uniformity)",
            ));
        }
        if name == "Untap" {
            return Err(Error::new_spanned(
                &variant.variant_name,
                "operation `Untap` collides with the macro-mandated observable close verb; rename the domain operation (the observability verbs `Tap`/`Untap` are fixed per /246 §2 for persona-introspect vocabulary uniformity)",
            ));
        }
    }
    for variant in &spec.reply.variants {
        if variant.variant_name == "ObserverSubscriptionOpened" {
            return Err(Error::new_spanned(
                &variant.variant_name,
                "reply variant `ObserverSubscriptionOpened` collides with the `observable` block's auto-injected reply variant",
            ));
        }
    }
    if let Some(event_block) = &spec.event {
        let observable_event_names: HashSet<String> = observable
            .event_records()
            .iter()
            .map(|event| event.to_string())
            .collect();
        for variant in &event_block.variants {
            let name = variant.variant_name.to_string();
            if observable_event_names.contains(&name) {
                return Err(Error::new_spanned(
                    &variant.variant_name,
                    format!(
                        "event variant `{name}` collides with the `observable` block's auto-injected event variant",
                    ),
                ));
            }
            let payload_head = projected_record_head(&variant.payload_type);
            if observable_event_names.contains(&payload_head) {
                return Err(Error::new_spanned(
                    &variant.variant_name,
                    format!(
                        "event variant `{name}` projects to record head `{payload_head}` which collides with the `observable` block's auto-injected event payload",
                    ),
                ));
            }
        }
    }
    let auto_stream_name = "ObserverStream".to_string();
    for stream in &spec.streams {
        if stream.name == auto_stream_name {
            return Err(Error::new_spanned(
                &stream.name,
                format!(
                    "stream `{auto_stream_name}` collides with the `observable` block's auto-injected stream",
                ),
            ));
        }
    }
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
        spec.reply.variants.iter().filter_map(|v| {
            v.payload_type
                .as_ref()
                .map(|payload| (&v.variant_name, payload))
        }),
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
                format!("stream `{stream_name}` is orphaned — no request operation opens it",),
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
        for event_variant_name in &stream.events {
            if !event_variants.contains(&event_variant_name.to_string()) {
                return Err(Error::new_spanned(
                    event_variant_name,
                    format!(
                        "stream `{}`: `event {}` does not resolve to a variant in the event block",
                        stream.name, event_variant_name,
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
                        .find(|variant| &variant.variant_name == event_variant_name)
                })
                .expect("event variant verified above");
            let event_belongs = event_variant
                .belongs
                .as_ref()
                .expect("event belongs relation verified above");
            if event_belongs != &stream.name {
                return Err(Error::new_spanned(
                    event_variant_name,
                    format!(
                        "stream `{}` names event `{}` but that event belongs to stream `{}`",
                        stream.name, event_variant_name, event_belongs,
                    ),
                ));
            }
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
