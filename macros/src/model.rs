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
    pub(crate) observable: Option<ObservableBlockSpec>,
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
    pub(crate) events: Vec<Ident>,
    pub(crate) close: Ident,
}

pub(crate) struct RequestVariantSpec {
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

/// Opt-in observer-subscription declaration. When present, the macro
/// injects `Observe` / `Unobserve` operations, an `ObserverStream`
/// (whose events carry the contract-author-supplied event payload
/// types), and an observer set / publish surface on the daemon side.
pub(crate) struct ObservableBlockSpec {
    /// Span pointer for diagnostics — points at the `observable`
    /// keyword.
    pub(crate) span: Ident,
    /// Contract-author-defined filter type. The macro references the
    /// name; the contract crate declares the type and implements
    /// `ObserverFilterMatch` against it.
    pub(crate) filter: Ident,
    /// Contract-author-defined event payload types, in declaration
    /// order. Each becomes a variant of the channel's event enum and
    /// an event slot on the macro-generated `ObserverStream`.
    pub(crate) events: Vec<Ident>,
}

impl ChannelSpec {
    pub(crate) fn is_streaming(&self) -> bool {
        self.event.is_some() || !self.streams.is_empty() || self.observable.is_some()
    }
}
