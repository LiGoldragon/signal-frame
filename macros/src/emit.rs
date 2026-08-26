//! Code emission. Generates the typed payload enums, kind enums,
//! `RequestPayload` impl, frame aliases, stream-relation witnesses,
//! and DOTOS codec impls. When the channel declaration carries an
//! `observable` block, also injects observer-subscription operations,
//! an `ObserverStream`, and the runtime publish surface.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::parse_quote;

use crate::model::{
    ChannelSpec, EventBlockSpec, EventVariantSpec, FilterDecl, ObservableBlockSpec, ReplyBlockSpec,
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
    let log_variant_impl = emit_log_variant_impl(&augmented);
    let operation_dispatch = emit_operation_dispatch(&augmented);
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
    let dotos_codecs = emit_dotos_codecs(&augmented);

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
        #dotos_codecs
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
        payload_type: Some(opened_type),
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
        contract: spec.contract.clone(),
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
        contract: spec.contract.clone(),
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
        #[rkyv(serialize_bounds(
            __S: ::rkyv::ser::Writer + ::rkyv::ser::Allocator,
            __S::Error: ::rkyv::rancor::Source,
        ))]
        #[rkyv(deserialize_bounds(__D::Error: ::rkyv::rancor::Source))]
        #[rkyv(bytecheck(bounds(__C: ::rkyv::validation::ArchiveContext)))]
        pub enum #name {
            #( #variants, )*
        }
    }
}

fn emit_reply_enum(block: &ReplyBlockSpec) -> TokenStream {
    let name = &block.name;
    let variants = block.variants.iter().map(|v| {
        let variant_name = &v.variant_name;
        match &v.payload_type {
            Some(payload) => quote! { #variant_name(#payload) },
            None => quote! { #variant_name },
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
        #[rkyv(serialize_bounds(
            __S: ::rkyv::ser::Writer + ::rkyv::ser::Allocator,
            __S::Error: ::rkyv::rancor::Source,
        ))]
        #[rkyv(deserialize_bounds(__D::Error: ::rkyv::rancor::Source))]
        #[rkyv(bytecheck(bounds(__C: ::rkyv::validation::ArchiveContext)))]
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
        #[rkyv(serialize_bounds(
            __S: ::rkyv::ser::Writer + ::rkyv::ser::Allocator,
            __S::Error: ::rkyv::rancor::Source,
        ))]
        #[rkyv(deserialize_bounds(__D::Error: ::rkyv::rancor::Source))]
        #[rkyv(bytecheck(bounds(__C: ::rkyv::validation::ArchiveContext)))]
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
            quote! {
                Self::#variant_name(payload) => {
                    bytes[0] = #index;
                    let _ = payload;
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

fn emit_operation_dispatch(spec: &ChannelSpec) -> TokenStream {
    let request = &spec.request;
    let reply = &spec.reply;
    let contract = &spec.contract;
    let request_name = &request.name;
    let reply_name = &reply.name;
    let request_kind_name = format_ident!("{}Kind", request_name);
    let validated_operation_name = format_ident!("Validated{}", request_name);
    let handler_trait_name = format_ident!("{}Handler", request_name);
    let dispatch_trait_name = format_ident!("{}Dispatch", request_name);

    quote! {
        /// Operation capability produced only after generated wire validation.
        pub struct #validated_operation_name {
            operation: #request_name,
        }

        impl #validated_operation_name {
            /// Borrow the operation after dispatch validation.
            pub fn as_operation(&self) -> &#request_name {
                &self.operation
            }

            /// Consume the capability wrapper and recover the typed operation.
            pub fn into_operation(self) -> #request_name {
                self.operation
            }

            /// Return the operation kind after dispatch validation.
            pub fn kind(&self) -> #request_kind_name {
                self.operation.kind()
            }

            fn from_dispatch(operation: #request_name) -> Self {
                Self { operation }
            }
        }

        // Trusted typed handling accepts only the private-constructor
        // capability wrapper, never an arbitrary decoded operation.
        #[allow(async_fn_in_trait)]
        pub trait #handler_trait_name {
            type Error;

            async fn handle_validated(
                &mut self,
                operation: #validated_operation_name,
            ) -> ::std::result::Result<#reply_name, Self::Error>;
        }

        // The sole public wire ingress. It validates binding, the complete
        // route, and archive/body header equality before trusted handling.
        #[allow(async_fn_in_trait)]
        pub trait #dispatch_trait_name: #handler_trait_name
        where
            Self::Error: From<::signal_frame::OperationDispatchError>
                + From<::signal_frame::FrameError>,
        {
            async fn dispatch(
                &mut self,
                frame: Frame,
            ) -> ::std::result::Result<#reply_name, Self::Error> {
                let short_header = frame.short_header();
                <#contract as ::signal_frame::WireContract>::BINDING
                    .validate_header(short_header)
                    .map_err(|error| <Self::Error as ::core::convert::From<::signal_frame::FrameError>>::from(error))?;

                let request = match frame.into_body() {
                    FrameBody::Request { request, .. } => request,
                    _ => {
                        return Err(::signal_frame::OperationDispatchError::UnexpectedFrameBody.into());
                    }
                };
                let (operation, tail) = request.payloads.into_head_and_tail();
                if !tail.is_empty() {
                    return Err(::signal_frame::OperationDispatchError::MultipleOperations {
                        count: 1 + tail.len(),
                    }
                    .into());
                }

                let actual_route = short_header.route();
                let expected_kind = #request_name::route_kind_from_short_header(short_header)
                    .map_err(<Self::Error as ::core::convert::From<::signal_frame::OperationDispatchError>>::from)?;
                let actual_kind = operation.kind();
                if actual_kind != expected_kind {
                    return Err(::signal_frame::OperationDispatchError::HeaderOperationMismatch {
                        expected: actual_route.root().value(),
                        actual: actual_kind as u8,
                    }
                    .into());
                }
                let expected_route = ::signal_frame::WireRoute::try_from_log_variant(
                        <#request_name as ::signal_frame::LogVariant>::log_variant(&operation),
                    )
                    .map_err(::signal_frame::FrameError::from)
                    .map_err(|error| <Self::Error as ::core::convert::From<::signal_frame::FrameError>>::from(error))?;
                if expected_route != actual_route {
                    return Err(::signal_frame::OperationDispatchError::HeaderRouteMismatch {
                        expected: expected_route,
                        actual: actual_route,
                    }
                    .into());
                }
                self.handle_validated(#validated_operation_name::from_dispatch(operation))
                    .await
            }
        }

        impl<Handler> #dispatch_trait_name for Handler
        where
            Handler: #handler_trait_name,
            Handler::Error: From<::signal_frame::OperationDispatchError>
                + From<::signal_frame::FrameError>,
        {
        }
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
    let route_arms = block.variants.iter().enumerate().map(|(index, v)| {
        let index = index as u8;
        let variant = &v.variant_name;
        quote! {
            (#index, 0) => Ok(#kind_name::#variant),
            (#index, variant) => Err(::signal_frame::OperationDispatchError::UnknownOperationVariant {
                root: #index,
                variant,
            }),
        }
    });
    quote! {
        #[cfg_attr(feature = "dotos-text", derive(::dotos::DotosEncode, ::dotos::DotosDecode))]
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
        pub enum #kind_name {
            #( #kind_variants, )*
        }

        impl #name {
            pub fn kind(&self) -> #kind_name {
                match self {
                    #( #arms, )*
                }
            }

            fn route_kind_from_short_header(
                short_header: ::signal_frame::ShortHeader,
            ) -> ::core::result::Result<
                #kind_name,
                ::signal_frame::OperationDispatchError,
            > {
                match (
                    short_header.route().root().value(),
                    short_header.route().variant().value(),
                ) {
                    #( #route_arms )*
                    _ => Err(::signal_frame::OperationDispatchError::UnknownOperationRoot {
                        root: short_header.route().root().value(),
                    }),
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
        if v.payload_type.is_some() {
            quote! { Self::#variant(_) => #kind_name::#variant }
        } else {
            quote! { Self::#variant => #kind_name::#variant }
        }
    });
    quote! {
        #[cfg_attr(feature = "dotos-text", derive(::dotos::DotosEncode, ::dotos::DotosDecode))]
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
        #[cfg_attr(feature = "dotos-text", derive(::dotos::DotosEncode, ::dotos::DotosDecode))]
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
        #[cfg_attr(feature = "dotos-text", derive(::dotos::DotosEncode, ::dotos::DotosDecode))]
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
    let contract = &spec.contract;

    if spec.is_streaming() {
        let event_name = &spec
            .event
            .as_ref()
            .expect("streaming channel has an event block per validate")
            .name;
        quote! {
            pub type #frame_alias =
                ::signal_frame::BoundStreamingFrame<#contract, #request_name, #reply_name, #event_name>;
            pub type #frame_body_alias =
                ::signal_frame::StreamingFrameBody<#request_name, #reply_name, #event_name>;
            pub type #channel_request_alias = ::signal_frame::Request<#request_name>;
            pub type #channel_reply_alias = ::signal_frame::Reply<#reply_name>;
            pub type #channel_builder_alias = ::signal_frame::RequestBuilder<#request_name>;
        }
    } else {
        quote! {
            pub type #frame_alias =
                ::signal_frame::BoundExchangeFrame<#contract, #request_name, #reply_name>;
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
            ) -> ::core::result::Result<Frame, ::signal_frame::WireRouteError> {
                let request = ::signal_frame::Request::from_payload(self);
                let route = request.route()?;
                Ok(Frame::new(route, FrameBody::Request { exchange, request }))
            }
        }
    }
}

fn emit_dotos_codecs(spec: &ChannelSpec) -> TokenStream {
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
    payload: Option<&'spec syn::Type>,
}

fn payload_kinds(block: &RequestBlockSpec) -> Vec<PayloadKind<'_>> {
    block
        .variants
        .iter()
        .map(|v| PayloadKind {
            variant: &v.variant_name,
            payload: Some(&v.payload_type),
        })
        .collect()
}

fn payload_kinds_reply(block: &ReplyBlockSpec) -> Vec<PayloadKind<'_>> {
    block
        .variants
        .iter()
        .map(|v| PayloadKind {
            variant: &v.variant_name,
            payload: v.payload_type.as_ref(),
        })
        .collect()
}

fn payload_kinds_event(block: &EventBlockSpec) -> Vec<PayloadKind<'_>> {
    block
        .variants
        .iter()
        .map(|v| PayloadKind {
            variant: &v.variant_name,
            payload: Some(&v.payload_type),
        })
        .collect()
}

fn emit_payload_enum_codec(name: &syn::Ident, kinds: Vec<PayloadKind<'_>>) -> TokenStream {
    let enum_name_string = name.to_string();
    let encode_arms = kinds.iter().map(|k| {
        let variant = &k.variant;
        let variant_string = variant.to_string();
        match k.payload {
            Some(payload) => quote! {
                Self::#variant(payload) => {
                    ::dotos::Delimiter::Parenthesis.wrap([
                        #variant_string.to_owned(),
                        <#payload as ::dotos::DotosEncode>::to_dotos(payload),
                    ])
                }
            },
            None => quote! {
                Self::#variant => #variant_string.to_owned()
            },
        }
    });
    let decode_payload_arms = kinds.iter().filter_map(|k| {
        let variant = &k.variant;
        let variant_string = variant.to_string();
        let payload = k.payload?;
        Some(quote! {
            #variant_string => {
                let payload = <#payload as ::dotos::DotosDecode>::from_dotos_block(
                    &children[1],
                )?;
                Ok(Self::#variant(payload))
            }
        })
    });
    let decode_unit_arms = kinds.iter().filter_map(|k| {
        let variant = &k.variant;
        let variant_string = variant.to_string();
        k.payload.is_none().then(|| {
            quote! {
                #variant_string => Ok(Self::#variant)
            }
        })
    });
    quote! {
        #[cfg(feature = "dotos-text")]
        impl ::dotos::DotosEncode for #name {
            fn to_dotos(&self) -> String {
                match self {
                    #( #encode_arms, )*
                }
            }
        }

        #[cfg(feature = "dotos-text")]
        impl ::dotos::DotosDecode for #name {
            fn from_dotos_block(
                block: &::dotos::Block,
            ) -> Result<Self, ::dotos::DotosDecodeError> {
                if let Some(head) = block.demote_to_string() {
                    return match head {
                        #( #decode_unit_arms, )*
                        other => Err(::dotos::DotosDecodeError::UnknownVariant {
                            enum_name: #enum_name_string,
                            variant: other.to_string(),
                        }),
                    };
                }

                let children = ::dotos::DotosBlock::new(block).expect_children(
                    ::dotos::Delimiter::Parenthesis,
                    #enum_name_string,
                    2,
                )?;
                let head = children[0]
                    .demote_to_string()
                    .ok_or(::dotos::DotosDecodeError::ExpectedAtom {
                        type_name: #enum_name_string,
                    })?;
                match head {
                    #( #decode_payload_arms, )*
                    other => Err(::dotos::DotosDecodeError::UnknownVariant {
                        enum_name: #enum_name_string,
                        variant: other.to_string(),
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
        /// prevents cross-channel token confusion. The DOTOS record
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

        #[cfg(feature = "dotos-text")]
        impl ::dotos::DotosEncode for #token_type_name {
            fn to_dotos(&self) -> String {
                ::dotos::Delimiter::Parenthesis.wrap([
                    "ObserverSubscriptionToken".to_owned(),
                    self.0.value().to_dotos(),
                ])
            }
        }

        #[cfg(feature = "dotos-text")]
        impl ::dotos::DotosDecode for #token_type_name {
            fn from_dotos_block(
                block: &::dotos::Block,
            ) -> Result<Self, ::dotos::DotosDecodeError> {
                let children = ::dotos::DotosBlock::new(block).expect_children(
                    ::dotos::Delimiter::Parenthesis,
                    "ObserverSubscriptionToken",
                    2,
                )?;
                let head = children[0]
                    .demote_to_string()
                    .ok_or(::dotos::DotosDecodeError::ExpectedAtom {
                        type_name: "ObserverSubscriptionToken",
                    })?;
                if head != "ObserverSubscriptionToken" {
                    return Err(::dotos::DotosDecodeError::UnknownVariant {
                        enum_name: "ObserverSubscriptionToken",
                        variant: head.to_owned(),
                    });
                }
                let value = u64::from_dotos_block(&children[1])?;
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

        #[cfg(feature = "dotos-text")]
        impl ::dotos::DotosEncode for #opened_type_name {
            fn to_dotos(&self) -> String {
                ::dotos::Delimiter::Parenthesis.wrap([
                    "ObserverSubscriptionOpened".to_owned(),
                    self.token.to_dotos(),
                ])
            }
        }

        #[cfg(feature = "dotos-text")]
        impl ::dotos::DotosDecode for #opened_type_name {
            fn from_dotos_block(
                block: &::dotos::Block,
            ) -> Result<Self, ::dotos::DotosDecodeError> {
                let children = ::dotos::DotosBlock::new(block).expect_children(
                    ::dotos::Delimiter::Parenthesis,
                    "ObserverSubscriptionOpened",
                    2,
                )?;
                let head = children[0]
                    .demote_to_string()
                    .ok_or(::dotos::DotosDecodeError::ExpectedAtom {
                        type_name: "ObserverSubscriptionOpened",
                    })?;
                if head != "ObserverSubscriptionOpened" {
                    return Err(::dotos::DotosDecodeError::UnknownVariant {
                        enum_name: "ObserverSubscriptionOpened",
                        variant: head.to_owned(),
                    });
                }
                let token = #token_type_name::from_dotos_block(&children[1])?;
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
    // the `OperationKind` enum with rkyv + DOTOS derives so it
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

        #[cfg(feature = "dotos-text")]
        impl ::dotos::DotosEncode for #filter_type {
            fn to_dotos(&self) -> String {
                match self {
                    Self::All => "All".to_owned(),
                    Self::OperationsOnly => "OperationsOnly".to_owned(),
                    Self::EffectsOnly => "EffectsOnly".to_owned(),
                }
            }
        }

        #[cfg(feature = "dotos-text")]
        impl ::dotos::DotosDecode for #filter_type {
            fn from_dotos_block(
                block: &::dotos::Block,
            ) -> Result<Self, ::dotos::DotosDecodeError> {
                let head = block
                    .demote_to_string()
                    .ok_or(::dotos::DotosDecodeError::ExpectedAtom {
                        type_name: stringify!(#filter_type),
                    })?;
                match head {
                    "All" => Ok(Self::All),
                    "OperationsOnly" => Ok(Self::OperationsOnly),
                    "EffectsOnly" => Ok(Self::EffectsOnly),
                    other => Err(::dotos::DotosDecodeError::UnknownVariant {
                        enum_name: stringify!(#filter_type),
                        variant: other.to_string(),
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
