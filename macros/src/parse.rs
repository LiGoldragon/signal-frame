//! `syn`-based parser for the `signal_channel!` channel declaration
//! grammar. Builds a [`crate::model::ChannelSpec`].

use syn::parse::{Parse, ParseStream};
use syn::{Ident, Result, Token, Type, braced};

use crate::model::{
    ChannelSpec, EventBlockSpec, EventVariantSpec, ObservableBlockSpec, ReplyBlockSpec,
    ReplyVariantSpec, RequestBlockSpec, RequestVariantSpec, StreamBlockSpec,
};

mod keyword {
    syn::custom_keyword!(channel);
    syn::custom_keyword!(operation);
    syn::custom_keyword!(reply);
    syn::custom_keyword!(event);
    syn::custom_keyword!(stream);
    syn::custom_keyword!(opens);
    syn::custom_keyword!(belongs);
    syn::custom_keyword!(token);
    syn::custom_keyword!(opened);
    syn::custom_keyword!(open);
    syn::custom_keyword!(close);
    syn::custom_keyword!(observable);
    syn::custom_keyword!(filter);
    syn::custom_keyword!(operation_event);
    syn::custom_keyword!(effect_event);
}

impl Parse for ChannelSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        input.parse::<keyword::channel>()?;
        let name = input.parse::<Ident>()?;

        let operation_body;
        braced!(operation_body in input);

        let request_name = Ident::new(&format!("{name}Operation"), name.span());
        let mut request_variants = Vec::new();
        while !operation_body.is_empty() {
            request_variants.push(operation_body.parse::<RequestVariantSpec>()?);
            if !operation_body.is_empty() {
                operation_body.parse::<Token![,]>()?;
            }
        }

        let request = RequestBlockSpec {
            name: request_name,
            variants: request_variants,
        };

        let mut reply: Option<ReplyBlockSpec> = None;
        let mut event: Option<EventBlockSpec> = None;
        let mut streams: Vec<StreamBlockSpec> = Vec::new();
        let mut observable: Option<ObservableBlockSpec> = None;

        while !input.is_empty() {
            let lookahead = input.lookahead1();
            if lookahead.peek(keyword::reply) {
                if reply.is_some() {
                    return Err(input.error("duplicate `reply` block"));
                }
                reply = Some(input.parse()?);
            } else if lookahead.peek(keyword::event) {
                if event.is_some() {
                    return Err(input.error("duplicate `event` block"));
                }
                event = Some(input.parse()?);
            } else if lookahead.peek(keyword::stream) {
                streams.push(input.parse()?);
            } else if lookahead.peek(keyword::observable) {
                if observable.is_some() {
                    return Err(input.error("duplicate `observable` block"));
                }
                observable = Some(input.parse()?);
            } else {
                return Err(lookahead.error());
            }
        }

        let reply = reply.ok_or_else(|| {
            syn::Error::new_spanned(&name, "channel declaration requires a `reply` block")
        })?;

        Ok(ChannelSpec {
            name,
            request,
            reply,
            event,
            streams,
            observable,
        })
    }
}

impl Parse for RequestBlockSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
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
        input.parse::<keyword::operation>()?;
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

        let mut events: Vec<Ident> = Vec::new();
        while body.peek(keyword::event) {
            body.parse::<keyword::event>()?;
            events.push(body.parse::<Ident>()?);
            body.parse::<Token![;]>()?;
        }
        if events.is_empty() {
            return Err(body.error("stream block requires at least one `event <Variant>;` slot"));
        }

        body.parse::<keyword::close>()?;
        let close = body.parse::<Ident>()?;
        body.parse::<Token![;]>()?;

        Ok(StreamBlockSpec {
            name,
            token,
            opened,
            events,
            close,
        })
    }
}

impl Parse for ObservableBlockSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let span_token = input.parse::<keyword::observable>()?;
        let body;
        braced!(body in input);

        // open <Verb>(<FilterType>);
        if !body.peek(keyword::open) {
            return Err(syn::Error::new(
                span_token.span,
                "observable block requires `open <Verb>(<FilterType>);` as its first declaration",
            ));
        }
        body.parse::<keyword::open>()?;
        let open_verb = body.parse::<Ident>()?;
        let open_payload;
        syn::parenthesized!(open_payload in body);
        let open_filter = open_payload.parse::<Ident>()?;
        body.parse::<Token![;]>()?;

        // close <Verb>;
        if !body.peek(keyword::close) {
            return Err(syn::Error::new(
                span_token.span,
                "observable block requires `close <Verb>;` after the `open` line; the close payload is the macro-generated `<Channel>ObserverSubscriptionToken`",
            ));
        }
        body.parse::<keyword::close>()?;
        let close_verb = body.parse::<Ident>()?;
        body.parse::<Token![;]>()?;

        // filter <FilterType>;
        if !body.peek(keyword::filter) {
            return Err(syn::Error::new(
                span_token.span,
                "observable block requires `filter <FilterType>;` after the `close` line",
            ));
        }
        body.parse::<keyword::filter>()?;
        let filter = body.parse::<Ident>()?;
        body.parse::<Token![;]>()?;

        if filter != open_filter {
            return Err(syn::Error::new(
                open_filter.span(),
                format!(
                    "filter type `{open_filter}` in `open {open_verb}({open_filter})` must match `filter {filter};` declaration",
                ),
            ));
        }

        // operation_event <Type>; effect_event <Type>;
        // The order matters because the macro uses them positionally
        // when wiring publish_operation_received vs publish_effect_emitted.
        if !body.peek(keyword::operation_event) {
            return Err(syn::Error::new(
                span_token.span,
                "observable block requires `operation_event <Type>;` after the `filter` line — the event record published before lowering",
            ));
        }
        body.parse::<keyword::operation_event>()?;
        let operation_event = body.parse::<Ident>()?;
        body.parse::<Token![;]>()?;

        if !body.peek(keyword::effect_event) {
            return Err(syn::Error::new(
                span_token.span,
                "observable block requires `effect_event <Type>;` after the `operation_event` line — the event record published after atomic commit",
            ));
        }
        body.parse::<keyword::effect_event>()?;
        let effect_event = body.parse::<Ident>()?;
        body.parse::<Token![;]>()?;

        if !body.is_empty() {
            return Err(body.error(
                "observable block accepts exactly two event roles: `operation_event` and `effect_event`",
            ));
        }

        Ok(ObservableBlockSpec {
            open_verb,
            close_verb,
            filter,
            operation_event,
            effect_event,
        })
    }
}
