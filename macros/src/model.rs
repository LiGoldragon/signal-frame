//! Typed model produced by [`crate::parse`] and consumed by
//! [`crate::validate`] / [`crate::emit`]. One `ChannelSpec` per
//! `signal_channel!` invocation.

use syn::{Ident, Type};

pub(crate) struct ChannelSpec {
    pub(crate) name: Ident,
    pub(crate) request: RequestBlockSpec,
    pub(crate) reply: ReplyBlockSpec,
    pub(crate) event: Option<EventBlockSpec>,
    pub(crate) streams: Vec<StreamBlockSpec>,
}

pub(crate) struct RequestBlockSpec {
    pub(crate) name: Ident,
    pub(crate) variants: Vec<RequestVariantSpec>,
}

pub(crate) struct ReplyBlockSpec {
    pub(crate) name: Ident,
    pub(crate) variants: Vec<ReplyVariantSpec>,
}

pub(crate) struct EventBlockSpec {
    pub(crate) name: Ident,
    pub(crate) variants: Vec<EventVariantSpec>,
}

pub(crate) struct StreamBlockSpec {
    pub(crate) name: Ident,
    pub(crate) token: Type,
    pub(crate) opened: Ident,
    pub(crate) event_variant: Ident,
    pub(crate) close: Ident,
}

pub(crate) struct RequestVariantSpec {
    pub(crate) verb_keyword: Ident,
    pub(crate) variant_name: Ident,
    pub(crate) payload_type: Type,
    pub(crate) opens: Option<Ident>,
}

pub(crate) struct ReplyVariantSpec {
    pub(crate) variant_name: Ident,
    pub(crate) payload_type: Type,
}

pub(crate) struct EventVariantSpec {
    pub(crate) variant_name: Ident,
    pub(crate) payload_type: Type,
    pub(crate) belongs: Option<Ident>,
}

impl ChannelSpec {
    pub(crate) fn is_streaming(&self) -> bool {
        self.event.is_some() || !self.streams.is_empty()
    }
}

pub(crate) const SIGNAL_VERBS: [&str; 6] = [
    "Assert",
    "Mutate",
    "Retract",
    "Match",
    "Subscribe",
    "Validate",
];
