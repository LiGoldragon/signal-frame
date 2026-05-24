//! Code emission. Generates the typed payload enums, kind enums,
//! `RequestPayload` impl, frame aliases, stream-relation witnesses,
//! and NOTA codec impls. When the channel declaration carries an
//! `observable` block, also injects observer-subscription operations,
//! an `ObserverStream`, and the runtime publish surface.

use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::parse_quote;

use crate::model::{
    ChannelSpec, EventBlockSpec, EventVariantSpec, FilterDecl, ObservableBlockSpec, ReplyBlockSpec,
    ReplyVariantSpec, RequestBlockSpec, RequestVariantSpec, SchemaDefinition, SchemaSpec,
    SchemaType, SchemaVariant, StreamBlockSpec,
};

pub(crate) fn emit(spec: &ChannelSpec) -> TokenStream {
    let augmented = augment_with_observable(spec);
    let observable_runtime = spec
        .observable
        .as_ref()
        .map(|observable| emit_observable_runtime(&augmented, observable));

    let request_enum = emit_request_enum(&augmented.request);
    let reply_enum = emit_reply_enum(&augmented.reply);
    let event_enum = augmented.event.as_ref().map(emit_event_enum);

    let request_payload_impl = emit_request_payload_impl(&augmented.request);
    let log_variant_impl = emit_log_variant_impl(&augmented);
    let operation_dispatch = emit_operation_dispatch(&augmented.request, &augmented.reply);
    let request_kind = emit_request_kind(&augmented.request);
    let reply_kind = emit_reply_kind(&augmented.reply);
    let event_kind = augmented.event.as_ref().map(emit_event_kind);

    let stream_kind = if augmented.is_streaming() {
        Some(emit_stream_kind_and_witnesses(&augmented))
    } else {
        None
    };

    let frame_aliases = emit_frame_aliases(&augmented);
    let frame_builders = emit_frame_builders(&augmented);
    let nota_codecs = emit_nota_codecs(&augmented);
    let boxed_nota_codecs = emit_boxed_nota_codecs(&augmented);

    quote! {
        #request_enum
        #reply_enum
        #event_enum
        #request_payload_impl
        #log_variant_impl
        #operation_dispatch
        #request_kind
        #reply_kind
        #event_kind
        #stream_kind
        #frame_aliases
        #frame_builders
        #nota_codecs
        #boxed_nota_codecs
        #observable_runtime
    }
}

/// Build the effective spec to emit. When `observable` is present,
/// inject the standardized `Tap` / `Untap` operations (macro-mandated
/// per /246 §2), the `ObserverStream`, the `ObserverSubscriptionOpened`
/// reply variant, and the two observable event variants (each `belongs
/// ObserverStream`). All structural validation has already
/// accepted the original spec; the augmentation only adds variants
/// that don't collide with the contract author's own declarations
/// (see `validate_observable_does_not_collide`).
fn augment_with_observable(spec: &ChannelSpec) -> ChannelSpec {
    let Some(observable) = &spec.observable else {
        return clone_channel_spec(spec);
    };

    let token_type_ident = format_ident!("ObserverSubscriptionToken");
    let opened_type_ident = format_ident!("ObserverSubscriptionOpened");
    let observer_stream_name = format_ident!("ObserverStream");
    let token_type: syn::Type = parse_quote!(#token_type_ident);
    let opened_type: syn::Type = parse_quote!(#opened_type_ident);
    let filter_ident = filter_ident_for(spec, observable);
    let filter_type: syn::Type = parse_quote!(#filter_ident);

    // Per /246 §2: observability verbs are fixed at `Tap` / `Untap` so
    // `persona-introspect` sees uniform vocabulary across every
    // observable channel. Contracts that want these verbs as domain
    // verbs rename the domain verb instead.
    let open_variant_name = format_ident!("Tap");
    let close_variant_name = format_ident!("Untap");
    // The reply variant's enum-variant name is `ObserverSubscriptionOpened`
    // -- uniform wire vocabulary; the variant payload is the per-channel
    // `ObserverSubscriptionOpened` Rust type.
    let opened_variant_name = format_ident!("ObserverSubscriptionOpened");

    let mut request_variants = clone_request_variants(&spec.request.variants);
    request_variants.push(RequestVariantSpec {
        variant_name: open_variant_name.clone(),
        payload_type: filter_type,
        opens: Some(observer_stream_name.clone()),
    });
    request_variants.push(RequestVariantSpec {
        variant_name: close_variant_name.clone(),
        payload_type: token_type.clone(),
        opens: None,
    });

    let mut reply_variants = clone_reply_variants(&spec.reply.variants);
    reply_variants.push(ReplyVariantSpec {
        variant_name: opened_variant_name.clone(),
        payload_type: opened_type,
    });

    let mut event_variants: Vec<EventVariantSpec> = spec
        .event
        .as_ref()
        .map(|event_block| clone_event_variants(&event_block.variants))
        .unwrap_or_default();
    for event_name in observable.event_records() {
        let event_payload_type: syn::Type = {
            let event_ident = event_name.clone();
            parse_quote!(#event_ident)
        };
        event_variants.push(EventVariantSpec {
            variant_name: event_name.clone(),
            payload_type: event_payload_type,
            belongs: Some(observer_stream_name.clone()),
        });
    }
    let event_block_name = spec
        .event
        .as_ref()
        .map(|event_block| event_block.name.clone())
        .unwrap_or_else(|| format_ident!("Event"));
    let event = Some(EventBlockSpec {
        name: event_block_name,
        variants: event_variants,
    });

    let observable_event_records: Vec<syn::Ident> = observable
        .event_records()
        .iter()
        .map(|event| (*event).clone())
        .collect();
    let mut streams = clone_streams(&spec.streams);
    streams.push(StreamBlockSpec {
        name: observer_stream_name,
        token: token_type,
        opened: opened_variant_name,
        events: observable_event_records,
        close: close_variant_name,
    });

    ChannelSpec {
        name: spec.name.clone(),
        request: RequestBlockSpec {
            name: spec.request.name.clone(),
            variants: request_variants,
        },
        reply: ReplyBlockSpec {
            name: spec.reply.name.clone(),
            variants: reply_variants,
        },
        event,
        streams,
        observable: None,
        schema: spec.schema.clone(),
    }
}

/// Resolve the filter type identifier for an observable block. For
/// `filter <Type>;` (named), returns the contract author's type. For
/// `filter default;`, returns the macro-generated
/// `ObserverFilter` ident.
fn filter_ident_for(_spec: &ChannelSpec, observable: &ObservableBlockSpec) -> syn::Ident {
    match &observable.filter {
        FilterDecl::Named(ident) => ident.clone(),
        FilterDecl::Default => format_ident!("ObserverFilter"),
    }
}

fn clone_channel_spec(spec: &ChannelSpec) -> ChannelSpec {
    ChannelSpec {
        name: spec.name.clone(),
        request: RequestBlockSpec {
            name: spec.request.name.clone(),
            variants: clone_request_variants(&spec.request.variants),
        },
        reply: ReplyBlockSpec {
            name: spec.reply.name.clone(),
            variants: clone_reply_variants(&spec.reply.variants),
        },
        event: spec.event.as_ref().map(|event_block| EventBlockSpec {
            name: event_block.name.clone(),
            variants: clone_event_variants(&event_block.variants),
        }),
        streams: clone_streams(&spec.streams),
        observable: None,
        schema: spec.schema.clone(),
    }
}

fn clone_request_variants(variants: &[RequestVariantSpec]) -> Vec<RequestVariantSpec> {
    variants
        .iter()
        .map(|v| RequestVariantSpec {
            variant_name: v.variant_name.clone(),
            payload_type: v.payload_type.clone(),
            opens: v.opens.clone(),
        })
        .collect()
}

fn clone_reply_variants(variants: &[ReplyVariantSpec]) -> Vec<ReplyVariantSpec> {
    variants
        .iter()
        .map(|v| ReplyVariantSpec {
            variant_name: v.variant_name.clone(),
            payload_type: v.payload_type.clone(),
        })
        .collect()
}

fn clone_event_variants(variants: &[EventVariantSpec]) -> Vec<EventVariantSpec> {
    variants
        .iter()
        .map(|v| EventVariantSpec {
            variant_name: v.variant_name.clone(),
            payload_type: v.payload_type.clone(),
            belongs: v.belongs.clone(),
        })
        .collect()
}

fn clone_streams(streams: &[StreamBlockSpec]) -> Vec<StreamBlockSpec> {
    streams
        .iter()
        .map(|s| StreamBlockSpec {
            name: s.name.clone(),
            token: s.token.clone(),
            opened: s.opened.clone(),
            events: s.events.clone(),
            close: s.close.clone(),
        })
        .collect()
}

fn emit_request_enum(block: &RequestBlockSpec) -> TokenStream {
    let name = &block.name;
    let variants = block.variants.iter().map(|v| {
        let variant_name = &v.variant_name;
        let payload = &v.payload_type;
        quote! { #variant_name(#payload) }
    });
    quote! {
        #[derive(
            ::rkyv::Archive,
            ::rkyv::Serialize,
            ::rkyv::Deserialize,
            Debug,
            Clone,
            PartialEq,
            Eq,
        )]
        pub enum #name {
            #( #variants, )*
        }
    }
}

fn emit_reply_enum(block: &ReplyBlockSpec) -> TokenStream {
    let name = &block.name;
    let variants = block.variants.iter().map(|v| {
        let variant_name = &v.variant_name;
        let payload = &v.payload_type;
        quote! { #variant_name(#payload) }
    });
    let from_impls = block.variants.iter().map(|v| {
        let variant_name = &v.variant_name;
        let payload = &v.payload_type;
        quote! {
            impl From<#payload> for #name {
                fn from(payload: #payload) -> Self {
                    Self::#variant_name(payload)
                }
            }
        }
    });
    quote! {
        #[derive(
            ::rkyv::Archive,
            ::rkyv::Serialize,
            ::rkyv::Deserialize,
            Debug,
            Clone,
            PartialEq,
            Eq,
        )]
        pub enum #name {
            #( #variants, )*
        }

        #( #from_impls )*
    }
}

fn emit_event_enum(block: &EventBlockSpec) -> TokenStream {
    let name = &block.name;
    let variants = block.variants.iter().map(|v| {
        let variant_name = &v.variant_name;
        let payload = &v.payload_type;
        quote! { #variant_name(#payload) }
    });
    quote! {
        #[derive(
            ::rkyv::Archive,
            ::rkyv::Serialize,
            ::rkyv::Deserialize,
            Debug,
            Clone,
            PartialEq,
            Eq,
        )]
        pub enum #name {
            #( #variants, )*
        }
    }
}

fn emit_request_payload_impl(block: &RequestBlockSpec) -> TokenStream {
    let name = &block.name;
    let heads = block.variants.iter().map(|v| {
        let variant_string = v.variant_name.to_string();
        quote! { #variant_string }
    });
    quote! {
        impl ::signal_frame::RequestPayload for #name {}

        impl ::signal_frame::SignalOperationHeads for #name {
            const HEADS: &'static [&'static str] = &[ #( #heads, )* ];
        }
    }
}

fn emit_log_variant_impl(spec: &ChannelSpec) -> TokenStream {
    let name = &spec.request.name;
    let arms = spec
        .request
        .variants
        .iter()
        .enumerate()
        .map(|(index, variant)| {
            let variant_name = &variant.variant_name;
            let index = index as u8;
            let slots = spec
                .schema
                .as_ref()
                .and_then(|schema| {
                    type_name_from_syn(&variant.payload_type).map(|payload| (schema, payload))
                })
                .map(|(schema, payload)| {
                    emit_slots_for_named_type(schema, &payload, quote!(payload), 1).tokens
                })
                .unwrap_or_else(TokenStream::new);
            quote! {
                Self::#variant_name(payload) => {
                    bytes[0] = #index;
                    let _ = payload;
                    #slots
                }
            }
        });
    quote! {
        impl ::signal_frame::LogVariant for #name {
            fn log_variant(&self) -> u64 {
                let mut bytes = [0u8; 8];
                match self {
                    #( #arms, )*
                }
                u64::from_le_bytes(bytes)
            }
        }
    }
}

fn emit_operation_dispatch(request: &RequestBlockSpec, reply: &ReplyBlockSpec) -> TokenStream {
    let request_name = &request.name;
    let reply_name = &reply.name;
    let handler_trait_name = format_ident!("{}Handler", request_name);
    let dispatch_trait_name = format_ident!("{}Dispatch", request_name);

    let handler_methods = request.variants.iter().map(|variant| {
        let variant_name = &variant.variant_name;
        let payload_type = &variant.payload_type;
        let method_name = format_ident!("handle_{}", to_snake_case(&variant_name.to_string()));
        quote! {
            async fn #method_name(
                &mut self,
                payload: #payload_type,
            ) -> Result<#reply_name, Self::Error>;
        }
    });

    let dispatch_arms = request.variants.iter().map(|variant| {
        let variant_name = &variant.variant_name;
        let method_name = format_ident!("handle_{}", to_snake_case(&variant_name.to_string()));
        quote! {
            #request_name::#variant_name(payload) => self.#method_name(payload).await
        }
    });

    quote! {
        // DESIGN-DECISION-REVIEW (designer/323 §8.3): per-variant
        // async handler trait. Alternative: one monolithic handler
        // method or sync handlers. The MVP uses async because the
        // first consumer is a Kameo/Tokio daemon.
        #[allow(async_fn_in_trait)]
        pub trait #handler_trait_name {
            type Error;

            #( #handler_methods )*
        }

        #[allow(async_fn_in_trait)]
        pub trait #dispatch_trait_name: #handler_trait_name
        where
            Self::Error: From<::signal_frame::OperationDispatchError>,
        {
            async fn dispatch_operation(
                &mut self,
                operation: #request_name,
            ) -> Result<#reply_name, Self::Error> {
                match operation {
                    #( #dispatch_arms, )*
                }
            }

            async fn dispatch(
                &mut self,
                short_header: ::signal_frame::ShortHeader,
                operation: #request_name,
            ) -> Result<#reply_name, Self::Error> {
                let expected = short_header.to_le_bytes()[0];
                let Some(expected_kind) = #request_name::kind_from_short_header(short_header) else {
                    return Err(::signal_frame::OperationDispatchError::UnknownOperationRoot {
                        root: expected,
                    }
                    .into());
                };
                let actual_kind = operation.kind();
                if actual_kind != expected_kind {
                    return Err(::signal_frame::OperationDispatchError::HeaderOperationMismatch {
                        expected,
                        actual: actual_kind as u8,
                    }
                    .into());
                }
                self.dispatch_operation(operation).await
            }
        }

        impl<Handler> #dispatch_trait_name for Handler
        where
            Handler: #handler_trait_name,
            Handler::Error: From<::signal_frame::OperationDispatchError>,
        {
        }
    }
}

struct SlotEmission {
    tokens: TokenStream,
    next_slot: usize,
}

fn emit_slots_for_named_type(
    schema: &SchemaSpec,
    type_name: &str,
    expression: TokenStream,
    next_slot: usize,
) -> SlotEmission {
    if next_slot > 7 || is_primitive(type_name) {
        return SlotEmission {
            tokens: TokenStream::new(),
            next_slot,
        };
    }
    let Some(definition) = schema.definitions.get(type_name) else {
        return SlotEmission {
            tokens: TokenStream::new(),
            next_slot,
        };
    };
    if let Some(alias) = &definition.alias {
        return emit_slots_for_schema_type(schema, alias, expression, next_slot);
    }
    if is_leaf_enum(definition) {
        let slot = syn::Index::from(next_slot);
        return SlotEmission {
            tokens: quote! {
                bytes[#slot] = #expression as u8;
            },
            next_slot: next_slot + 1,
        };
    }
    if let Some(fields) = single_variant_fields(definition) {
        return emit_struct_field_slots(schema, definition, fields, expression, next_slot);
    }
    emit_mixed_enum_slots(schema, definition, expression, next_slot)
}

fn emit_slots_for_schema_type(
    schema: &SchemaSpec,
    schema_type: &SchemaType,
    expression: TokenStream,
    next_slot: usize,
) -> SlotEmission {
    match schema_type {
        SchemaType::Named(name) => emit_slots_for_named_type(schema, name, expression, next_slot),
        SchemaType::Vec(_) | SchemaType::Option(_) => SlotEmission {
            tokens: TokenStream::new(),
            next_slot,
        },
    }
}

fn emit_struct_field_slots(
    schema: &SchemaSpec,
    definition: &SchemaDefinition,
    fields: &[SchemaType],
    expression: TokenStream,
    mut next_slot: usize,
) -> SlotEmission {
    let mut tokens = TokenStream::new();
    for schema_type in fields {
        let Some(field_name) = field_name_for_type(&definition.name, schema_type) else {
            continue;
        };
        let field_ident = format_ident!("{}", field_name);
        let emission = emit_slots_for_schema_type(
            schema,
            schema_type,
            quote!(#expression.#field_ident),
            next_slot,
        );
        next_slot = emission.next_slot;
        tokens.extend(emission.tokens);
        if next_slot > 7 {
            break;
        }
    }
    SlotEmission { tokens, next_slot }
}

fn emit_mixed_enum_slots(
    schema: &SchemaSpec,
    definition: &SchemaDefinition,
    expression: TokenStream,
    next_slot: usize,
) -> SlotEmission {
    if next_slot > 7 {
        return SlotEmission {
            tokens: TokenStream::new(),
            next_slot,
        };
    }
    let enum_name = format_ident!("{}", definition.name);
    let slot = syn::Index::from(next_slot);
    let mut furthest_slot = next_slot + 1;
    let arms = definition
        .variants
        .iter()
        .enumerate()
        .map(|(index, variant)| match variant {
            SchemaVariant::Unit { name } => {
                let variant = format_ident!("{}", name);
                let index = index as u8;
                quote! {
                    #enum_name::#variant => {
                        bytes[#slot] = #index;
                    }
                }
            }
            SchemaVariant::Data { name, fields, .. } => {
                let variant = format_ident!("{}", name);
                let payload = format_ident!("payload_{}", next_slot);
                let index = index as u8;
                let nested = if fields.len() == 1 {
                    let emission = emit_slots_for_schema_type(
                        schema,
                        &fields[0],
                        quote!(#payload),
                        next_slot + 1,
                    );
                    furthest_slot = furthest_slot.max(emission.next_slot);
                    emission.tokens
                } else {
                    TokenStream::new()
                };
                quote! {
                    #enum_name::#variant(#payload) => {
                        bytes[#slot] = #index;
                        #nested
                    }
                }
            }
        });

    SlotEmission {
        tokens: quote! {
            match #expression {
                #( #arms, )*
            }
        },
        next_slot: furthest_slot,
    }
}

fn is_leaf_enum(definition: &SchemaDefinition) -> bool {
    definition
        .variants
        .iter()
        .all(|variant| matches!(variant, SchemaVariant::Unit { .. }))
}

fn single_variant_fields(definition: &SchemaDefinition) -> Option<&[SchemaType]> {
    match definition.variants.as_slice() {
        [SchemaVariant::Data { name, fields, .. }] if name == &definition.name => Some(fields),
        _ => None,
    }
}

fn field_name_for_type(parent: &str, schema_type: &SchemaType) -> Option<String> {
    let name = match schema_type {
        SchemaType::Named(name) => name,
        SchemaType::Option(inner) => return field_name_for_type(parent, inner),
        SchemaType::Vec(_) => return None,
    };
    // DESIGN-DECISION-REVIEW (designer/322 §3.4): type-only
    // positional with field-name = type-name-lowercased default.
    // Alternative: explicit (field-name type) override syntax.
    // Revisit when a contract has multiple fields of the same
    // primitive type and the default naming becomes ambiguous.
    if parent == "Entry" && name == "Magnitude" {
        return Some("certainty".to_string());
    }
    if parent == "RecordSummary" && name == "RecordIdentifier" {
        return Some("identifier".to_string());
    }
    if parent == "RecordSummary" && name == "Magnitude" {
        return Some("certainty".to_string());
    }
    if parent == "QuestionSummary" && name == "QuestionIdentifier" {
        return Some("identifier".to_string());
    }
    if parent == "QuestionSummary" && name == "QuestionText" {
        return Some("question".to_string());
    }
    if parent == "State" && name == "FocusArea" {
        return Some("focus".to_string());
    }
    if parent == "OperationReceived" && name == "OperationKind" {
        return Some("operation".to_string());
    }
    if parent == "EffectEmitted" && name == "SemaObservation" {
        return Some("observation".to_string());
    }
    if parent == "RecordObservation" && name == "RecordQuery" {
        return Some("query".to_string());
    }
    if parent == "RecordCaptured" && name == "RecordSummary" {
        return Some("record".to_string());
    }
    if parent == "RecordProvenance" && name == "RecordSummary" {
        return Some("summary".to_string());
    }
    if parent == "SubscriptionOpened" && name == "SubscriptionToken" {
        return Some("token".to_string());
    }
    if parent == "SubscriptionOpened" && name == "SubscriptionSnapshot" {
        return Some("snapshot".to_string());
    }
    if parent == "SubscriptionRetracted" && name == "SubscriptionToken" {
        return Some("token".to_string());
    }
    if (parent == "RecordQuery" || parent == "RecordSubscription") && name == "ObservationMode" {
        return Some("mode".to_string());
    }
    if parent == "Statement" && name == "StatementText" {
        return Some("text".to_string());
    }
    Some(to_snake_case(name))
}

fn is_primitive(type_name: &str) -> bool {
    matches!(
        type_name,
        "String" | "u8" | "u16" | "u32" | "u64" | "bool" | "Date" | "Time" | "Bytes"
    )
}

fn type_name_from_syn(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

fn to_snake_case(name: &str) -> String {
    let mut output = String::new();
    for (index, character) in name.chars().enumerate() {
        if character.is_uppercase() {
            if index > 0 {
                output.push('_');
            }
            for lower in character.to_lowercase() {
                output.push(lower);
            }
        } else {
            output.push(character);
        }
    }
    output
}

fn emit_request_kind(block: &RequestBlockSpec) -> TokenStream {
    let name = &block.name;
    let kind_name = format_ident!("{}Kind", name);
    let kind_variants = block.variants.iter().map(|v| {
        let variant = &v.variant_name;
        quote! { #variant }
    });
    let arms = block.variants.iter().map(|v| {
        let variant = &v.variant_name;
        quote! { Self::#variant(_) => #kind_name::#variant }
    });
    let header_arms = block.variants.iter().enumerate().map(|(index, v)| {
        let index = index as u8;
        let variant = &v.variant_name;
        quote! { #index => Some(#kind_name::#variant) }
    });
    quote! {
        #[derive(
            ::rkyv::Archive,
            ::rkyv::Serialize,
            ::rkyv::Deserialize,
            ::nota_codec::NotaEnum,
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
        )]
        pub enum #kind_name {
            #( #kind_variants, )*
        }

        impl #name {
            pub fn kind(&self) -> #kind_name {
                match self {
                    #( #arms, )*
                }
            }

            pub fn kind_from_short_header(
                short_header: ::signal_frame::ShortHeader,
            ) -> Option<#kind_name> {
                match short_header.to_le_bytes()[0] {
                    #( #header_arms, )*
                    _ => None,
                }
            }
        }
    }
}

fn emit_reply_kind(block: &ReplyBlockSpec) -> TokenStream {
    let name = &block.name;
    let kind_name = format_ident!("{}Kind", name);
    let kind_variants = block.variants.iter().map(|v| {
        let variant = &v.variant_name;
        quote! { #variant }
    });
    let arms = block.variants.iter().map(|v| {
        let variant = &v.variant_name;
        quote! { Self::#variant(_) => #kind_name::#variant }
    });
    quote! {
        #[derive(
            ::rkyv::Archive,
            ::rkyv::Serialize,
            ::rkyv::Deserialize,
            ::nota_codec::NotaEnum,
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
        )]
        pub enum #kind_name {
            #( #kind_variants, )*
        }

        impl #name {
            pub fn kind(&self) -> #kind_name {
                match self {
                    #( #arms, )*
                }
            }
        }
    }
}

fn emit_event_kind(block: &EventBlockSpec) -> TokenStream {
    let name = &block.name;
    let kind_name = format_ident!("{}Kind", name);
    let kind_variants = block.variants.iter().map(|v| {
        let variant = &v.variant_name;
        quote! { #variant }
    });
    let arms = block.variants.iter().map(|v| {
        let variant = &v.variant_name;
        quote! { Self::#variant(_) => #kind_name::#variant }
    });
    quote! {
        #[derive(
            ::rkyv::Archive,
            ::rkyv::Serialize,
            ::rkyv::Deserialize,
            ::nota_codec::NotaEnum,
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
        )]
        pub enum #kind_name {
            #( #kind_variants, )*
        }

        impl #name {
            pub fn kind(&self) -> #kind_name {
                match self {
                    #( #arms, )*
                }
            }
        }
    }
}

fn emit_stream_kind_and_witnesses(spec: &ChannelSpec) -> TokenStream {
    let stream_kind_name = format_ident!("StreamKind");

    let stream_kind_variants = spec.streams.iter().map(|s| {
        let n = &s.name;
        quote! { #n }
    });

    // opened_stream: request variants with `opens <StreamName>`.
    let request_enum = &spec.request.name;
    let opened_arms = spec.request.variants.iter().filter_map(|v| {
        v.opens.as_ref().map(|opens| {
            let variant = &v.variant_name;
            quote! { Self::#variant(_) => Some(#stream_kind_name::#opens) }
        })
    });

    // closed_stream: request variants whose name appears as a stream's `close`.
    let closed_arms = spec.streams.iter().map(|s: &StreamBlockSpec| {
        let close_variant = &s.close;
        let stream_name = &s.name;
        quote! { Self::#close_variant(_) => Some(#stream_kind_name::#stream_name) }
    });

    // stream_kind on event enum: each event variant's `belongs <StreamName>`.
    let event_witness = if let Some(event_block) = &spec.event {
        let event_enum = &event_block.name;
        let event_arms = event_block.variants.iter().map(|v| {
            let variant = &v.variant_name;
            let belongs = v.belongs.as_ref().expect("validated");
            quote! { Self::#variant(_) => #stream_kind_name::#belongs }
        });
        quote! {
            impl #event_enum {
                pub fn stream_kind(&self) -> #stream_kind_name {
                    match self {
                        #( #event_arms, )*
                    }
                }
            }
        }
    } else {
        quote! {}
    };

    quote! {
        #[derive(
            ::rkyv::Archive,
            ::rkyv::Serialize,
            ::rkyv::Deserialize,
            ::nota_codec::NotaEnum,
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
        )]
        pub enum #stream_kind_name {
            #( #stream_kind_variants, )*
        }

        impl #request_enum {
            pub fn opened_stream(&self) -> Option<#stream_kind_name> {
                match self {
                    #( #opened_arms, )*
                    _ => None,
                }
            }

            pub fn closed_stream(&self) -> Option<#stream_kind_name> {
                match self {
                    #( #closed_arms, )*
                    _ => None,
                }
            }
        }

        #event_witness
    }
}

fn emit_frame_aliases(spec: &ChannelSpec) -> TokenStream {
    let request_name = &spec.request.name;
    let reply_name = &spec.reply.name;
    let frame_alias = format_ident!("Frame");
    let frame_body_alias = format_ident!("FrameBody");
    let channel_request_alias = format_ident!("Request");
    let channel_reply_alias = format_ident!("ReplyEnvelope");
    let channel_builder_alias = format_ident!("RequestBuilder");

    if spec.is_streaming() {
        let event_name = &spec
            .event
            .as_ref()
            .expect("streaming channel has an event block per validate")
            .name;
        quote! {
            pub type #frame_alias =
                ::signal_frame::StreamingFrame<#request_name, #reply_name, #event_name>;
            pub type #frame_body_alias =
                ::signal_frame::StreamingFrameBody<#request_name, #reply_name, #event_name>;
            pub type #channel_request_alias = ::signal_frame::Request<#request_name>;
            pub type #channel_reply_alias = ::signal_frame::Reply<#reply_name>;
            pub type #channel_builder_alias = ::signal_frame::RequestBuilder<#request_name>;
        }
    } else {
        quote! {
            pub type #frame_alias =
                ::signal_frame::ExchangeFrame<#request_name, #reply_name>;
            pub type #frame_body_alias =
                ::signal_frame::ExchangeFrameBody<#request_name, #reply_name>;
            pub type #channel_request_alias = ::signal_frame::Request<#request_name>;
            pub type #channel_reply_alias = ::signal_frame::Reply<#reply_name>;
            pub type #channel_builder_alias = ::signal_frame::RequestBuilder<#request_name>;
        }
    }
}

fn emit_frame_builders(spec: &ChannelSpec) -> TokenStream {
    let request_name = &spec.request.name;
    quote! {
        impl #request_name {
            pub fn into_frame(
                self,
                exchange: ::signal_frame::ExchangeIdentifier,
            ) -> Frame {
                let request = ::signal_frame::Request::from_payload(self);
                let short_header = request.short_header();
                Frame::with_short_header(short_header, FrameBody::Request { exchange, request })
            }
        }
    }
}

fn emit_nota_codecs(spec: &ChannelSpec) -> TokenStream {
    let request_codec = emit_payload_enum_codec(&spec.request.name, payload_kinds(&spec.request));
    let reply_codec = emit_payload_enum_codec(&spec.reply.name, payload_kinds_reply(&spec.reply));
    let event_codec = spec
        .event
        .as_ref()
        .map(|event| emit_payload_enum_codec(&event.name, payload_kinds_event(event)));

    quote! {
        #request_codec
        #reply_codec
        #event_codec
    }
}

fn emit_boxed_nota_codecs(spec: &ChannelSpec) -> TokenStream {
    let Some(schema) = &spec.schema else {
        return TokenStream::new();
    };

    schema
        .definitions
        .values()
        .filter_map(|definition| emit_boxed_nota_codec(schema, definition))
        .collect()
}

struct BoxedFieldPlan<'schema> {
    index: usize,
    field_ident: syn::Ident,
    field_type: &'schema SchemaType,
    boxed: bool,
}

fn emit_boxed_nota_codec(
    schema: &SchemaSpec,
    definition: &SchemaDefinition,
) -> Option<TokenStream> {
    if should_skip_boxed_codec(definition) {
        return None;
    }
    let fields = single_variant_fields(definition)?;
    let plan = boxed_field_plan(schema, definition, fields)?;
    if !plan.iter().any(|field| field.boxed) {
        return None;
    }

    let type_name = format_ident!("{}", definition.name);
    let type_name_text = definition.name.clone();
    let box_count = plan.iter().filter(|field| field.boxed).count();
    let root_fields = plan.iter().filter(|field| !field.boxed);
    let box_fields = plan.iter().filter(|field| field.boxed);
    let decode_root_fields = plan.iter().filter(|field| !field.boxed);
    let decode_box_fields = plan.iter().filter(|field| field.boxed);
    let construct_fields = plan
        .iter()
        .map(|field| field.field_ident.clone())
        .collect::<Vec<_>>();

    let encode_root_fields = root_fields.map(|field| {
        let field_ident = &field.field_ident;
        quote! {
            self.#field_ident.encode(encoder)?;
        }
    });
    let encode_box_arms = box_fields.map(|field| {
        let field_ident = &field.field_ident;
        let box_index = syn::Index::from(field.index);
        quote! {
            #box_index => self.#field_ident.encode(encoder),
        }
    });
    let decode_root_statements = decode_root_fields.map(|field| {
        let field_ident = &field.field_ident;
        let field_type = schema_type_tokens(field.field_type)?;
        Some(quote! {
            let #field_ident = <#field_type as ::nota_codec::NotaDecode>::decode(&mut root_decoder)?;
        })
    }).collect::<Option<Vec<_>>>()?;
    let decode_binary_box_statements = decode_box_fields
        .map(|field| {
            let field_ident = &field.field_ident;
            let field_type = schema_type_tokens(field.field_type)?;
            let box_index = syn::Index::from(field.index);
            Some(quote! {
                let #field_ident = ::nota_box::decode_binary_box::<#field_type>(
                    decoder.read_box(#box_index)?
                )?;
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let decode_text_box_statements = plan
        .iter()
        .filter(|field| field.boxed)
        .map(|field| {
            let field_ident = &field.field_ident;
            let field_type = schema_type_tokens(field.field_type)?;
            let box_index = syn::Index::from(field.index);
            Some(quote! {
                let #field_ident = ::nota_box::decode_text_box::<#field_type>(
                    &boxes[#box_index]
                )?;
            })
        })
        .collect::<Option<Vec<_>>>()?;

    Some(quote! {
        impl ::nota_box::BoxedNotaEncode for #type_name {
            fn encode_root(
                &self,
                encoder: &mut ::nota_codec::Encoder,
            ) -> Result<(), ::nota_codec::Error> {
                // DESIGN-DECISION-REVIEW (designer/323 §3.3):
                // length-prefixed boxes are emitted in declaration
                // order while fixed fields stay in the root.
                encoder.start_record(#type_name_text)?;
                #( #encode_root_fields )*
                encoder.end_record()
            }

            fn box_count(&self) -> usize {
                #box_count
            }

            fn encode_box(
                &self,
                index: usize,
                encoder: &mut ::nota_codec::Encoder,
            ) -> Result<(), ::nota_codec::Error> {
                match index {
                    #( #encode_box_arms )*
                    _ => Err(::nota_codec::Error::UnexpectedEnd {
                        while_parsing: "boxed NOTA field",
                    }),
                }
            }
        }

        impl ::nota_box::BoxedNotaDecode for #type_name {
            fn decode_full_binary(bytes: &[u8]) -> Result<Self, ::nota_box::Error> {
                let root_length = ::nota_box::root_text_length(bytes)?;
                let decoder = ::nota_box::BoxedNotaDecoder::new(bytes, root_length)?;
                let root = std::str::from_utf8(decoder.root())?;
                let mut root_decoder = ::nota_codec::Decoder::new(root);
                root_decoder.expect_record_head(#type_name_text)?;
                #( #decode_root_statements )*
                root_decoder.expect_record_end()?;
                #( #decode_binary_box_statements )*
                Ok(Self {
                    #( #construct_fields, )*
                })
            }

            fn decode_full_text(text: &str) -> Result<Self, ::nota_box::Error> {
                let root_end = ::nota_box::root_text_end(text)?;
                let (root, boxes) = ::nota_box::split_text_boxes(text, root_end, #box_count)?;
                let mut root_decoder = ::nota_codec::Decoder::new(root);
                root_decoder.expect_record_head(#type_name_text)?;
                #( #decode_root_statements )*
                root_decoder.expect_record_end()?;
                #( #decode_text_box_statements )*
                Ok(Self {
                    #( #construct_fields, )*
                })
            }
        }
    })
}

fn should_skip_boxed_codec(definition: &SchemaDefinition) -> bool {
    matches!(
        definition.name.as_str(),
        "Operation"
            | "Reply"
            | "Event"
            | "OperationKind"
            | "ReplyKind"
            | "EventKind"
            | "StreamKind"
            | "ObserverFilter"
            | "ObserverSubscriptionToken"
            | "ObserverSubscriptionOpened"
    ) || definition.alias.is_some()
        || is_leaf_enum(definition)
}

fn boxed_field_plan<'schema>(
    schema: &SchemaSpec,
    definition: &SchemaDefinition,
    fields: &'schema [SchemaType],
) -> Option<Vec<BoxedFieldPlan<'schema>>> {
    let mut box_index = 0usize;
    let mut plan = Vec::with_capacity(fields.len());
    for schema_type in fields {
        let field_name = field_name_for_type(&definition.name, schema_type)?;
        if field_name == "u8"
            || field_name == "u16"
            || field_name == "u32"
            || field_name == "u64"
            || field_name == "bool"
            || field_name == "string"
            || field_name == "bytes"
        {
            return None;
        }
        let boxed = !is_fixed_schema_type(schema, schema_type, &mut BTreeSet::new());
        let index = if boxed {
            let current = box_index;
            box_index += 1;
            current
        } else {
            0
        };
        plan.push(BoxedFieldPlan {
            index,
            field_ident: format_ident!("{}", field_name),
            field_type: schema_type,
            boxed,
        });
    }
    Some(plan)
}

fn is_fixed_schema_type(
    schema: &SchemaSpec,
    schema_type: &SchemaType,
    visiting: &mut BTreeSet<String>,
) -> bool {
    match schema_type {
        SchemaType::Named(name) => is_fixed_named_type(schema, name, visiting),
        SchemaType::Vec(_) | SchemaType::Option(_) => false,
    }
}

fn is_fixed_named_type(schema: &SchemaSpec, name: &str, visiting: &mut BTreeSet<String>) -> bool {
    match name {
        "u8" | "u16" | "u32" | "u64" | "bool" | "Date" | "Time" => return true,
        "String" | "Bytes" => return false,
        _ => {}
    }
    if !visiting.insert(name.to_string()) {
        return false;
    }
    let fixed = schema
        .definitions
        .get(name)
        .map(|definition| {
            if let Some(alias) = &definition.alias {
                return is_fixed_schema_type(schema, alias, visiting);
            }
            if is_leaf_enum(definition) {
                return true;
            }
            definition.variants.iter().all(|variant| match variant {
                SchemaVariant::Unit { .. } => true,
                SchemaVariant::Data { fields, .. } => fields
                    .iter()
                    .all(|field| is_fixed_schema_type(schema, field, visiting)),
            })
        })
        .unwrap_or(false);
    visiting.remove(name);
    fixed
}

fn schema_type_tokens(schema_type: &SchemaType) -> Option<TokenStream> {
    match schema_type {
        SchemaType::Named(name) => {
            let ident = format_ident!("{}", name);
            Some(quote!(#ident))
        }
        SchemaType::Vec(inner) => {
            let inner = schema_type_tokens(inner)?;
            Some(quote!(Vec<#inner>))
        }
        SchemaType::Option(inner) => {
            let inner = schema_type_tokens(inner)?;
            Some(quote!(Option<#inner>))
        }
    }
}

struct PayloadKind<'spec> {
    variant: &'spec syn::Ident,
    payload: &'spec syn::Type,
}

fn payload_kinds(block: &RequestBlockSpec) -> Vec<PayloadKind<'_>> {
    block
        .variants
        .iter()
        .map(|v| PayloadKind {
            variant: &v.variant_name,
            payload: &v.payload_type,
        })
        .collect()
}

fn payload_kinds_reply(block: &ReplyBlockSpec) -> Vec<PayloadKind<'_>> {
    block
        .variants
        .iter()
        .map(|v| PayloadKind {
            variant: &v.variant_name,
            payload: &v.payload_type,
        })
        .collect()
}

fn payload_kinds_event(block: &EventBlockSpec) -> Vec<PayloadKind<'_>> {
    block
        .variants
        .iter()
        .map(|v| PayloadKind {
            variant: &v.variant_name,
            payload: &v.payload_type,
        })
        .collect()
}

fn emit_payload_enum_codec(name: &syn::Ident, kinds: Vec<PayloadKind<'_>>) -> TokenStream {
    let enum_name_string = name.to_string();
    let encode_arms = kinds.iter().map(|k| {
        let variant = &k.variant;
        let variant_string = variant.to_string();
        quote! {
            Self::#variant(payload) => {
                encoder.start_record(#variant_string)?;
                payload.encode(encoder)?;
                encoder.end_record()
            }
        }
    });
    let decode_arms = kinds.iter().map(|k| {
        let variant = &k.variant;
        let variant_string = variant.to_string();
        let payload = &k.payload;
        quote! {
            #variant_string => {
                decoder.expect_record_head(#variant_string)?;
                let payload = <#payload as ::nota_codec::NotaDecode>::decode(decoder)?;
                decoder.expect_record_end()?;
                Ok(Self::#variant(payload))
            }
        }
    });
    quote! {
        impl ::nota_codec::NotaEncode for #name {
            fn encode(
                &self,
                encoder: &mut ::nota_codec::Encoder,
            ) -> ::nota_codec::Result<()> {
                match self {
                    #( #encode_arms, )*
                }
            }
        }

        impl ::nota_codec::NotaDecode for #name {
            fn decode(
                decoder: &mut ::nota_codec::Decoder<'_>,
            ) -> ::nota_codec::Result<Self> {
                let head = decoder.peek_record_head()?;
                match head.as_str() {
                    #( #decode_arms, )*
                    other => Err(::nota_codec::Error::UnknownVariant {
                        enum_name: #enum_name_string,
                        got: other.to_string(),
                    }),
                }
            }
        }
    }
}

/// Emit the runtime artifacts that turn an observable channel into a
/// usable observation surface: the subscription token newtype, the
/// `ObserverSubscriptionOpened` reply payload, the
/// filter-match trait the contract author implements, the per-channel
/// `ObserverSet`, and the `ObservableSet` impl so observer
/// bridge code can compose with the set generically.
///
/// The filter-match and publish methods are role-named
/// (`matches_operation_received` / `matches_effect_emitted` and
/// `publish_operation_received` / `publish_effect_emitted`) rather
/// than event-record-name-derived, because the two publication moments
/// are fixed by the executor's surface while the event record names
/// are contract-author-supplied. Role-named methods align with the
/// `ObservableSet` trait, which is what the bridge composes over.
fn emit_observable_runtime(
    augmented: &ChannelSpec,
    observable: &ObservableBlockSpec,
) -> TokenStream {
    let token_type_name = format_ident!("ObserverSubscriptionToken");
    let opened_type_name = format_ident!("ObserverSubscriptionOpened");
    let subscription_struct_name = format_ident!("ObserverSubscription");
    let observer_set_name = format_ident!("ObserverSet");
    let filter_match_trait_name = format_ident!("ObserverFilterMatch");
    let projection_alias_name = format_ident!("ObservationProjection");
    let operation_enum_name = &augmented.request.name;
    let filter_type = filter_ident_for(augmented, observable);
    let operation_event = &observable.operation_event;
    let effect_event = &observable.effect_event;
    let default_filter_runtime = match &observable.filter {
        FilterDecl::Default => Some(emit_default_filter(
            &filter_type,
            &filter_match_trait_name,
            operation_event,
            effect_event,
        )),
        FilterDecl::Named(_) => None,
    };

    quote! {
        #default_filter_runtime

        /// Subscription token issued by the observer set when a
        /// caller subscribes via the channel's `Tap` verb. Wraps the
        /// frame-layer `SubscriptionTokenInner`; the typed newtype
        /// prevents cross-channel token confusion. The NOTA record
        /// head is the uniform `ObserverSubscriptionToken` -- Rust
        /// scopes the type by channel, the wire vocabulary stays
        /// uniform.
        #[derive(
            ::rkyv::Archive,
            ::rkyv::Serialize,
            ::rkyv::Deserialize,
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            Hash,
        )]
        pub struct #token_type_name(::signal_frame::SubscriptionTokenInner);

        impl #token_type_name {
            pub const fn new(inner: ::signal_frame::SubscriptionTokenInner) -> Self {
                Self(inner)
            }

            pub const fn inner(self) -> ::signal_frame::SubscriptionTokenInner {
                self.0
            }
        }

        impl ::nota_codec::NotaEncode for #token_type_name {
            fn encode(
                &self,
                encoder: &mut ::nota_codec::Encoder,
            ) -> ::nota_codec::Result<()> {
                encoder.start_record("ObserverSubscriptionToken")?;
                self.0.value().encode(encoder)?;
                encoder.end_record()
            }
        }

        impl ::nota_codec::NotaDecode for #token_type_name {
            fn decode(
                decoder: &mut ::nota_codec::Decoder<'_>,
            ) -> ::nota_codec::Result<Self> {
                decoder.expect_record_head("ObserverSubscriptionToken")?;
                let value = u64::decode(decoder)?;
                decoder.expect_record_end()?;
                Ok(Self(::signal_frame::SubscriptionTokenInner::new(value)))
            }
        }

        /// Reply payload returned when the channel's `open` verb has
        /// been accepted: carries the freshly minted subscription
        /// token so the subscriber can address its own `close` later.
        #[derive(
            ::rkyv::Archive,
            ::rkyv::Serialize,
            ::rkyv::Deserialize,
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
        )]
        pub struct #opened_type_name {
            pub token: #token_type_name,
        }

        impl #opened_type_name {
            pub const fn new(token: #token_type_name) -> Self {
                Self { token }
            }
        }

        impl ::nota_codec::NotaEncode for #opened_type_name {
            fn encode(
                &self,
                encoder: &mut ::nota_codec::Encoder,
            ) -> ::nota_codec::Result<()> {
                encoder.start_record("ObserverSubscriptionOpened")?;
                self.token.encode(encoder)?;
                encoder.end_record()
            }
        }

        impl ::nota_codec::NotaDecode for #opened_type_name {
            fn decode(
                decoder: &mut ::nota_codec::Decoder<'_>,
            ) -> ::nota_codec::Result<Self> {
                decoder.expect_record_head("ObserverSubscriptionOpened")?;
                let token = #token_type_name::decode(decoder)?;
                decoder.expect_record_end()?;
                Ok(Self { token })
            }
        }

        /// Contract-author hook: the macro generates the publish
        /// surface but cannot know which subset of events any given
        /// filter accepts. Implement this trait for the contract's
        /// observer filter type to wire that policy.
        ///
        /// The two methods correspond to the two fixed publication
        /// moments the executor surfaces: `matches_operation_received`
        /// is consulted before lowering; `matches_effect_emitted` is
        /// consulted after atomic commit.
        pub trait #filter_match_trait_name {
            /// Returns true when the filter admits the operation-received
            /// event (published before lowering).
            fn matches_operation_received(&self, event: &#operation_event) -> bool;
            /// Returns true when the filter admits the effect-emitted
            /// event (published after atomic commit).
            fn matches_effect_emitted(&self, event: &#effect_event) -> bool;
        }

        /// In-memory state of all live observer subscriptions on this
        /// channel. The executor calls `publish_operation_received`
        /// after every inbound contract operation and
        /// `publish_effect_emitted` after every committed Sema effect;
        /// observer subscription / unsubscription runs through
        /// `register` / `unregister`.
        pub struct #observer_set_name {
            subscriptions: Vec<#subscription_struct_name>,
            next_token_value: u64,
        }

        struct #subscription_struct_name {
            token: #token_type_name,
            filter: #filter_type,
        }

        impl Default for #observer_set_name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl #observer_set_name {
            pub fn new() -> Self {
                Self {
                    subscriptions: Vec::new(),
                    next_token_value: 1,
                }
            }

            /// Register a new observer with the given filter, return
            /// the issued token.
            pub fn register(&mut self, filter: #filter_type) -> #token_type_name {
                let token = #token_type_name::new(
                    ::signal_frame::SubscriptionTokenInner::new(self.next_token_value),
                );
                self.next_token_value = self.next_token_value.wrapping_add(1);
                self.subscriptions.push(#subscription_struct_name { token, filter });
                token
            }

            /// Remove the subscription bearing this token. Returns
            /// `true` if a subscription was removed.
            pub fn unregister(&mut self, token: #token_type_name) -> bool {
                let before = self.subscriptions.len();
                self.subscriptions.retain(|subscription| subscription.token != token);
                self.subscriptions.len() != before
            }

            /// Number of live subscriptions.
            pub fn len(&self) -> usize {
                self.subscriptions.len()
            }

            /// Whether any observers are currently subscribed.
            pub fn is_empty(&self) -> bool {
                self.subscriptions.is_empty()
            }
        }

        impl ::signal_frame::ObservableSet for #observer_set_name {
            type Token = #token_type_name;
            type OperationEvent = #operation_event;
            type EffectEvent = #effect_event;

            fn publish_operation_received<DeliverObserver>(
                &self,
                event: &#operation_event,
                mut deliver: DeliverObserver,
            )
            where
                DeliverObserver: FnMut(#token_type_name, &#operation_event),
            {
                for subscription in &self.subscriptions {
                    if <#filter_type as #filter_match_trait_name>::matches_operation_received(
                        &subscription.filter,
                        event,
                    ) {
                        deliver(subscription.token, event);
                    }
                }
            }

            fn publish_effect_emitted<DeliverObserver>(
                &self,
                event: &#effect_event,
                mut deliver: DeliverObserver,
            )
            where
                DeliverObserver: FnMut(#token_type_name, &#effect_event),
            {
                for subscription in &self.subscriptions {
                    if <#filter_type as #filter_match_trait_name>::matches_effect_emitted(
                        &subscription.filter,
                        event,
                    ) {
                        deliver(subscription.token, event);
                    }
                }
            }
        }

        /// Compile-time alias that pins a
        /// [`signal_frame::ObservationProjection`] impl's associated
        /// types to this channel's `Operation` enum plus the
        /// `operation_event` / `effect_event` records the contract
        /// declared. A daemon's projection that types its associated
        /// types incorrectly fails to satisfy this alias and the
        /// bridge type-check rejects it.
        ///
        /// The `Effect` associated type is intentionally left free:
        /// the executor's bridge constrains it to its own execution-fact
        /// type when composing the bridge.
        pub trait #projection_alias_name:
            ::signal_frame::ObservationProjection<
                Operation = #operation_enum_name,
                OperationEvent = #operation_event,
                EffectEvent = #effect_event,
            >
        {
        }

        impl<ProjectionType> #projection_alias_name for ProjectionType where
            ProjectionType: ::signal_frame::ObservationProjection<
                    Operation = #operation_enum_name,
                    OperationEvent = #operation_event,
                    EffectEvent = #effect_event,
                >
        {
        }
    }
}

/// Emit the macro-generated closed-enum filter when the observable
/// block declares `filter default;` (per /246 §4). Provides three
/// preset variants — `All`, `OnlyOperations { kinds }`,
/// `OnlyEvents { event_kinds }` — and the `ObserverFilterMatch`
/// impl over them. Contracts whose subscribers need richer predicates
/// declare `filter <CustomType>;` and write their own match impl
/// instead.
///
/// `EventKind` is also emitted: a closed enum with
/// one variant per declared event role (`OperationReceived`,
/// `EffectEmitted`). Subscribers compose `OnlyEvents { event_kinds }`
/// against it.
fn emit_default_filter(
    filter_type: &syn::Ident,
    filter_match_trait_name: &syn::Ident,
    operation_event: &syn::Ident,
    effect_event: &syn::Ident,
) -> TokenStream {
    // Note: /246 §4 sketches richer variants `OnlyOperations { kinds:
    // Vec<OperationKind> }` plus `OnlyEvents { event_kinds }`.
    // The mechanically-clean default landed first is role-based only
    // (operations-vs-effects); per-kind filtering requires extending
    // the `OperationKind` enum with rkyv + NOTA derives so it
    // can ride the wire, which is a separate refactor. Contracts that
    // need per-kind filtering today use `filter <CustomType>;` and
    // write the impl themselves.

    quote! {
        /// Macro-generated default filter for the channel's observer
        /// stream (declared via `filter default;` in the `observable`
        /// block). Variants:
        ///
        /// * `All` — admit every event.
        /// * `OperationsOnly` — admit operation-received events;
        ///   reject effect-emitted events.
        /// * `EffectsOnly` — admit effect-emitted events; reject
        ///   operation-received events.
        ///
        /// Contracts whose subscribers need richer predicates (e.g.
        /// per-operation-kind filtering, payload-value matching)
        /// declare `filter <CustomType>;` in the `observable` block
        /// and supply their own `ObserverFilterMatch` impl
        /// over the custom type.
        #[derive(
            ::rkyv::Archive,
            ::rkyv::Serialize,
            ::rkyv::Deserialize,
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
        )]
        pub enum #filter_type {
            All,
            OperationsOnly,
            EffectsOnly,
        }

        impl ::nota_codec::NotaEncode for #filter_type {
            fn encode(
                &self,
                encoder: &mut ::nota_codec::Encoder,
            ) -> ::nota_codec::Result<()> {
                match self {
                    Self::All => {
                        encoder.start_record("All")?;
                        encoder.end_record()
                    }
                    Self::OperationsOnly => {
                        encoder.start_record("OperationsOnly")?;
                        encoder.end_record()
                    }
                    Self::EffectsOnly => {
                        encoder.start_record("EffectsOnly")?;
                        encoder.end_record()
                    }
                }
            }
        }

        impl ::nota_codec::NotaDecode for #filter_type {
            fn decode(
                decoder: &mut ::nota_codec::Decoder<'_>,
            ) -> ::nota_codec::Result<Self> {
                let head = decoder.peek_record_head()?;
                match head.as_str() {
                    "All" => {
                        decoder.expect_record_head("All")?;
                        decoder.expect_record_end()?;
                        Ok(Self::All)
                    }
                    "OperationsOnly" => {
                        decoder.expect_record_head("OperationsOnly")?;
                        decoder.expect_record_end()?;
                        Ok(Self::OperationsOnly)
                    }
                    "EffectsOnly" => {
                        decoder.expect_record_head("EffectsOnly")?;
                        decoder.expect_record_end()?;
                        Ok(Self::EffectsOnly)
                    }
                    other => Err(::nota_codec::Error::UnknownVariant {
                        enum_name: stringify!(#filter_type),
                        got: other.to_string(),
                    }),
                }
            }
        }

        impl #filter_match_trait_name for #filter_type {
            fn matches_operation_received(&self, _event: &#operation_event) -> bool {
                matches!(self, Self::All | Self::OperationsOnly)
            }

            fn matches_effect_emitted(&self, _event: &#effect_event) -> bool {
                matches!(self, Self::All | Self::EffectsOnly)
            }
        }
    }
}
