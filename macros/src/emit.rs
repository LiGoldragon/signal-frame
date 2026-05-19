//! Code emission. Generates the typed payload enums, kind enums,
//! `RequestPayload` impl, frame aliases, stream-relation witnesses,
//! and NOTA codec impls. When the channel declaration carries an
//! `observable` block, also injects observer-subscription operations,
//! an `ObserverStream`, and the runtime publish surface.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::parse_quote;

use crate::model::{
    ChannelSpec, EventBlockSpec, EventVariantSpec, ObservableBlockSpec, ReplyBlockSpec,
    ReplyVariantSpec, RequestBlockSpec, RequestVariantSpec, StreamBlockSpec,
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
    let request_kind = emit_request_kind(&augmented.request);
    let reply_kind = emit_reply_kind(&augmented.reply);
    let event_kind = augmented.event.as_ref().map(emit_event_kind);

    let stream_kind = if augmented.is_streaming() {
        Some(emit_stream_kind_and_witnesses(&augmented))
    } else {
        None
    };

    let frame_aliases = emit_frame_aliases(&augmented);
    let nota_codecs = emit_nota_codecs(&augmented);

    quote! {
        #request_enum
        #reply_enum
        #event_enum
        #request_payload_impl
        #request_kind
        #reply_kind
        #event_kind
        #stream_kind
        #frame_aliases
        #nota_codecs
        #observable_runtime
    }
}

/// Build the effective spec to emit. When `observable` is present,
/// inject the observer-subscription operations, the `ObserverStream`,
/// the `ObserverSubscriptionOpened` reply variant, and the observable
/// event variants (each `belongs ObserverStream`). All structural
/// validation has already accepted the original spec; the augmentation
/// only adds variants that don't collide with the contract author's
/// own declarations (see `validate_observable_does_not_collide`).
fn augment_with_observable(spec: &ChannelSpec) -> ChannelSpec {
    let Some(observable) = &spec.observable else {
        return clone_channel_spec(spec);
    };

    let span = observable.span.span();
    let channel = &spec.name;
    let token_type_ident = format_ident!("{}ObserverSubscriptionToken", channel);
    let opened_type_ident = format_ident!("{}ObserverSubscriptionOpened", channel);
    let token_type: syn::Type = parse_quote!(#token_type_ident);
    let opened_type: syn::Type = parse_quote!(#opened_type_ident);
    let filter_type = {
        let filter_ident = &observable.filter;
        let filter_type: syn::Type = parse_quote!(#filter_ident);
        filter_type
    };

    let observer_stream_name = ident("ObserverStream", span);
    let observe_variant_name = ident("Observe", span);
    let unobserve_variant_name = ident("Unobserve", span);
    // The reply variant's enum-variant name is `ObserverSubscriptionOpened`
    // — uniform wire vocabulary; the variant payload is the per-channel
    // `<Channel>ObserverSubscriptionOpened` Rust type.
    let opened_variant_name = ident("ObserverSubscriptionOpened", span);

    let mut request_variants = clone_request_variants(&spec.request.variants);
    request_variants.push(RequestVariantSpec {
        variant_name: observe_variant_name.clone(),
        payload_type: filter_type,
        opens: Some(observer_stream_name.clone()),
    });
    request_variants.push(RequestVariantSpec {
        variant_name: unobserve_variant_name,
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
    for event_name in &observable.events {
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
        .unwrap_or_else(|| ident(&format!("{}Event", spec.name), spec.name.span()));
    let event = Some(EventBlockSpec {
        name: event_block_name,
        variants: event_variants,
    });

    let mut streams = clone_streams(&spec.streams);
    streams.push(StreamBlockSpec {
        name: observer_stream_name,
        token: token_type,
        opened: opened_variant_name,
        events: observable.events.clone(),
        close: ident("Unobserve", span),
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
    }
}

fn ident(text: &str, span: Span) -> syn::Ident {
    syn::Ident::new(text, span)
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
    quote! {
        impl ::signal_frame::RequestPayload for #name {}
    }
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
    quote! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    let channel = &spec.name;
    let stream_kind_name = format_ident!("{}StreamKind", channel);

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
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    let channel = &spec.name;
    let request_name = &spec.request.name;
    let reply_name = &spec.reply.name;
    let frame_alias = format_ident!("{}Frame", channel);
    let frame_body_alias = format_ident!("{}FrameBody", channel);
    let channel_request_alias = format_ident!("{}ChannelRequest", channel);
    let channel_reply_alias = format_ident!("{}ChannelReply", channel);
    let channel_builder_alias = format_ident!("{}RequestBuilder", channel);

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
                    other => Err(::nota_codec::Error::UnknownKindForVerb {
                        verb: #enum_name_string,
                        got: other.to_string(),
                    }),
                }
            }
        }
    }
}

/// Emit the runtime artifacts that turn an observable channel into a
/// usable observation surface: the subscription token newtype, the
/// `ObserverSubscriptionOpened` reply payload, the filter-match trait
/// the contract author implements, and the per-channel `ObserverSet`
/// that the daemon's executor calls `publish_*` on.
fn emit_observable_runtime(
    augmented: &ChannelSpec,
    observable: &ObservableBlockSpec,
) -> TokenStream {
    let channel = &augmented.name;
    let token_type_name = format_ident!("{}ObserverSubscriptionToken", channel);
    let opened_type_name = format_ident!("{}ObserverSubscriptionOpened", channel);
    let subscription_struct_name = format_ident!("{}ObserverSubscription", channel);
    let observer_set_name = format_ident!("{}ObserverSet", channel);
    let filter_match_trait_name = format_ident!("{}ObserverFilterMatch", channel);
    let filter_type = &observable.filter;

    let event_idents: Vec<&syn::Ident> = observable.events.iter().collect();
    let trait_methods = event_idents.iter().map(|event| {
        let method_name = format_ident!("matches_{}", event_snake_case(event));
        quote! {
            fn #method_name(&self, event: &#event) -> bool;
        }
    });

    let publish_methods = event_idents.iter().map(|event| {
        let publish_name = format_ident!("publish_{}", event_snake_case(event));
        let match_method_name = format_ident!("matches_{}", event_snake_case(event));
        quote! {
            /// Deliver the event to every subscribed observer whose
            /// filter accepts it. `deliver` runs once per matching
            /// observer, in registration order.
            pub fn #publish_name<DeliverObserver>(
                &self,
                event: &#event,
                mut deliver: DeliverObserver,
            )
            where
                DeliverObserver: FnMut(#token_type_name, &#event),
            {
                for subscription in &self.subscriptions {
                    if <#filter_type as #filter_match_trait_name>::#match_method_name(
                        &subscription.filter,
                        event,
                    ) {
                        deliver(subscription.token, event);
                    }
                }
            }
        }
    });

    quote! {
        /// Subscription token issued by the observer set when a
        /// caller subscribes via `Observe`. Wraps the frame-layer
        /// `SubscriptionTokenInner`; the typed newtype prevents
        /// cross-channel token confusion. The NOTA record head is
        /// the uniform `ObserverSubscriptionToken` — Rust scopes the
        /// type by channel, the wire vocabulary stays uniform.
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

        /// Reply payload returned when an `Observe` call has been
        /// accepted: carries the freshly minted subscription token so
        /// the subscriber can address its own `Unobserve` later.
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
        pub trait #filter_match_trait_name {
            #( #trait_methods )*
        }

        /// In-memory state of all live observer subscriptions on this
        /// channel. The executor calls `publish_*` after the relevant
        /// daemon-side moment; observer subscription / unsubscription
        /// runs through `register` / `unregister`.
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

            #( #publish_methods )*
        }
    }
}

fn event_snake_case(ident: &syn::Ident) -> String {
    let camel = ident.to_string();
    let mut snake = String::with_capacity(camel.len() + camel.len() / 4);
    for (index, character) in camel.chars().enumerate() {
        if character.is_uppercase() {
            if index != 0 {
                snake.push('_');
            }
            for lowered in character.to_lowercase() {
                snake.push(lowered);
            }
        } else {
            snake.push(character);
        }
    }
    snake
}
