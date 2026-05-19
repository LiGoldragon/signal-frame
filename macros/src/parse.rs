//! `syn`-based parser for the `signal_channel!` channel declaration
//! grammar. Builds a [`crate::model::ChannelSpec`].

use syn::parse::{Parse, ParseStream};
use syn::{Ident, Result, Token, Type, braced};

use crate::model::{
    ChannelSpec, EventBlockSpec, EventVariantSpec, ReplyBlockSpec, ReplyVariantSpec,
    RequestBlockSpec, RequestVariantSpec, StreamBlockSpec,
};

mod keyword {
    syn::custom_keyword!(channel);
    syn::custom_keyword!(request);
    syn::custom_keyword!(reply);
    syn::custom_keyword!(event);
    syn::custom_keyword!(stream);
    syn::custom_keyword!(opens);
    syn::custom_keyword!(belongs);
    syn::custom_keyword!(token);
    syn::custom_keyword!(opened);
    syn::custom_keyword!(close);
}

impl Parse for ChannelSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        input.parse::<keyword::channel>()?;
        let name = input.parse::<Ident>()?;
        let body;
        braced!(body in input);

        let mut request: Option<RequestBlockSpec> = None;
        let mut reply: Option<ReplyBlockSpec> = None;
        let mut event: Option<EventBlockSpec> = None;
        let mut streams: Vec<StreamBlockSpec> = Vec::new();

        while !body.is_empty() {
            let lookahead = body.lookahead1();
            if lookahead.peek(keyword::request) {
                if request.is_some() {
                    return Err(body.error("duplicate `request` block"));
                }
                request = Some(body.parse()?);
            } else if lookahead.peek(keyword::reply) {
                if reply.is_some() {
                    return Err(body.error("duplicate `reply` block"));
                }
                reply = Some(body.parse()?);
            } else if lookahead.peek(keyword::event) {
                if event.is_some() {
                    return Err(body.error("duplicate `event` block"));
                }
                event = Some(body.parse()?);
            } else if lookahead.peek(keyword::stream) {
                streams.push(body.parse()?);
            } else {
                return Err(lookahead.error());
            }
        }

        let request = request.ok_or_else(|| {
            syn::Error::new_spanned(&name, "channel declaration requires a `request` block")
        })?;
        let reply = reply.ok_or_else(|| {
            syn::Error::new_spanned(&name, "channel declaration requires a `reply` block")
        })?;

        Ok(ChannelSpec {
            name,
            request,
            reply,
            event,
            streams,
        })
    }
}

impl Parse for RequestBlockSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        input.parse::<keyword::request>()?;
        let name = input.parse::<Ident>()?;
        let body;
        braced!(body in input);

        let mut variants = Vec::new();
        while !body.is_empty() {
            variants.push(body.parse::<RequestVariantSpec>()?);
            if !body.is_empty() {
                body.parse::<Token![,]>()?;
            }
        }
        Ok(RequestBlockSpec { name, variants })
    }
}

impl Parse for RequestVariantSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let verb_keyword = input.parse::<Ident>()?;
        let variant_name = input.parse::<Ident>()?;
        let payload;
        syn::parenthesized!(payload in input);
        let payload_type = payload.parse::<Type>()?;
        let opens = if input.peek(keyword::opens) {
            input.parse::<keyword::opens>()?;
            Some(input.parse::<Ident>()?)
        } else {
            None
        };
        Ok(RequestVariantSpec {
            verb_keyword,
            variant_name,
            payload_type,
            opens,
        })
    }
}

impl Parse for ReplyBlockSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        input.parse::<keyword::reply>()?;
        let name = input.parse::<Ident>()?;
        let body;
        braced!(body in input);

        let mut variants = Vec::new();
        while !body.is_empty() {
            variants.push(body.parse::<ReplyVariantSpec>()?);
            if !body.is_empty() {
                body.parse::<Token![,]>()?;
            }
        }
        Ok(ReplyBlockSpec { name, variants })
    }
}

impl Parse for ReplyVariantSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let variant_name = input.parse::<Ident>()?;
        let payload;
        syn::parenthesized!(payload in input);
        let payload_type = payload.parse::<Type>()?;
        Ok(ReplyVariantSpec {
            variant_name,
            payload_type,
        })
    }
}

impl Parse for EventBlockSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        input.parse::<keyword::event>()?;
        let name = input.parse::<Ident>()?;
        let body;
        braced!(body in input);

        let mut variants = Vec::new();
        while !body.is_empty() {
            variants.push(body.parse::<EventVariantSpec>()?);
            if !body.is_empty() {
                body.parse::<Token![,]>()?;
            }
        }
        Ok(EventBlockSpec { name, variants })
    }
}

impl Parse for EventVariantSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let variant_name = input.parse::<Ident>()?;
        let payload;
        syn::parenthesized!(payload in input);
        let payload_type = payload.parse::<Type>()?;
        let belongs = if input.peek(keyword::belongs) {
            input.parse::<keyword::belongs>()?;
            Some(input.parse::<Ident>()?)
        } else {
            None
        };
        Ok(EventVariantSpec {
            variant_name,
            payload_type,
            belongs,
        })
    }
}

impl Parse for StreamBlockSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        input.parse::<keyword::stream>()?;
        let name = input.parse::<Ident>()?;
        let body;
        braced!(body in input);

        body.parse::<keyword::token>()?;
        let token = body.parse::<Type>()?;
        body.parse::<Token![;]>()?;

        body.parse::<keyword::opened>()?;
        let opened = body.parse::<Ident>()?;
        body.parse::<Token![;]>()?;

        body.parse::<keyword::event>()?;
        let event_variant = body.parse::<Ident>()?;
        body.parse::<Token![;]>()?;

        body.parse::<keyword::close>()?;
        let close = body.parse::<Ident>()?;
        body.parse::<Token![;]>()?;

        Ok(StreamBlockSpec {
            name,
            token,
            opened,
            event_variant,
            close,
        })
    }
}
