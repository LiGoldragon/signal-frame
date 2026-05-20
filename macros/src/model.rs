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
/// injects two contract-author-named operations (open and close), an
/// `ObserverStream` (whose events carry the contract-author-supplied
/// event payload types), and an observer set / publish surface on the
/// daemon side.
///
/// The contract author names the open and close verbs so the macro
/// reserves no globally-shared verb name. The event grammar splits
/// `operation_event <Type>;` from `effect_event <Type>;` so the macro
/// knows which event record maps to which publication moment
/// (`publish_operation_received` vs `publish_effect_emitted`).
pub(crate) struct ObservableBlockSpec {
    /// Contract-author-named verb for opening an observer
    /// subscription. Becomes `operation <OpenVerb>(<FilterType>)
    /// opens ObserverStream` in the emitted request enum.
    pub(crate) open_verb: Ident,
    /// Contract-author-named verb for closing an observer subscription.
    /// Becomes `operation <CloseVerb>(<Channel>ObserverSubscriptionToken)`
    /// in the emitted request enum. The token payload is macro-determined.
    pub(crate) close_verb: Ident,
    /// Contract-author-defined filter type. The macro references the
    /// name; the contract crate declares the type and implements
    /// `<Channel>ObserverFilterMatch` against it.
    pub(crate) filter: Ident,
    /// Contract-author-defined event record type that names the
    /// `OperationReceived` publication moment (executor pre-lowering).
    /// Exactly one per observable block.
    pub(crate) operation_event: Ident,
    /// Contract-author-defined event record type that names the
    /// `SemaEffectEmitted` publication moment (executor post-commit).
    /// Exactly one per observable block.
    pub(crate) effect_event: Ident,
}

impl ObservableBlockSpec {
    /// Enumerate every event record this block declares. Used by the
    /// emit pass when synthesising the channel's event enum and the
    /// `<Channel>ObserverStream` block.
    pub(crate) fn event_records(&self) -> [&Ident; 2] {
        [&self.operation_event, &self.effect_event]
    }
}

impl ChannelSpec {
    pub(crate) fn is_streaming(&self) -> bool {
        self.event.is_some() || !self.streams.is_empty() || self.observable.is_some()
    }
}
