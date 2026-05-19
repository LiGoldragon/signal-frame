//! Code emission. Generates the typed payload enums, kind enums,
//! `RequestPayload` impl, frame aliases, stream-relation witnesses,
//! and NOTA codec impls.
//!
//! NOTE: This emitter still encodes the pre-migration verb-tagged
//! shape — it emits `::signal_frame::SignalVerb` and
//! `signal_verb()` references that do not exist in signal-frame.
//! The full redesign to contract-local verbs is deferred; see the
//! MUST IMPLEMENT block in `lib.rs` and `../README.md`.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::model::{
    ChannelSpec, EventBlockSpec, ReplyBlockSpec, RequestBlockSpec, StreamBlockSpec,
};

pub(crate) fn emit(spec: &ChannelSpec) -> TokenStream {
    let request_enum = emit_request_enum(&spec.request);
    let reply_enum = emit_reply_enum(&spec.reply);
    let event_enum = spec.event.as_ref().map(emit_event_enum);

    let request_payload_impl = emit_request_payload_impl(&spec.request);
    let request_kind = emit_request_kind(&spec.request);
    let reply_kind = emit_reply_kind(&spec.reply);
    let event_kind = spec.event.as_ref().map(emit_event_kind);

    let stream_kind = if spec.is_streaming() {
        Some(emit_stream_kind_and_witnesses(spec))
    } else {
        None
    };

    let frame_aliases = emit_frame_aliases(spec);
    let nota_codecs = emit_nota_codecs(spec);

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
    }
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
    let arms = block.variants.iter().map(|v| {
        let variant = &v.variant_name;
        let verb = &v.verb_keyword;
        quote! { Self::#variant(_) => ::signal_frame::SignalVerb::#verb }
    });
    quote! {
        impl ::signal_frame::RequestPayload for #name {
            fn signal_verb(&self) -> ::signal_frame::SignalVerb {
                match self {
                    #( #arms, )*
                }
            }
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
