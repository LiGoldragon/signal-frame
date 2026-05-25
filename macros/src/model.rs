//! Typed model produced by [`crate::parse`] and consumed by
//! [`crate::validate`] / [`crate::emit`]. One `ChannelSpec` per
//! `signal_channel!` invocation.

use std::collections::BTreeMap;

use syn::{Ident, Type};

#[derive(Clone)]
pub(crate) struct ChannelSpec {
    pub(crate) name: Ident,
    pub(crate) request: RequestBlockSpec,
    pub(crate) reply: ReplyBlockSpec,
    pub(crate) event: Option<EventBlockSpec>,
    pub(crate) streams: Vec<StreamBlockSpec>,
    pub(crate) observable: Option<ObservableBlockSpec>,
    pub(crate) schema: Option<SchemaSpec>,
}

#[derive(Clone)]
pub(crate) struct RequestBlockSpec {
    pub(crate) name: Ident,
    pub(crate) variants: Vec<RequestVariantSpec>,
}

#[derive(Clone)]
pub(crate) struct ReplyBlockSpec {
    pub(crate) name: Ident,
    pub(crate) variants: Vec<ReplyVariantSpec>,
}

#[derive(Clone)]
pub(crate) struct EventBlockSpec {
    pub(crate) name: Ident,
    pub(crate) variants: Vec<EventVariantSpec>,
}

#[derive(Clone)]
pub(crate) struct StreamBlockSpec {
    pub(crate) name: Ident,
    pub(crate) token: Type,
    pub(crate) opened: Ident,
    pub(crate) events: Vec<Ident>,
    pub(crate) close: Ident,
}

#[derive(Clone)]
pub(crate) struct RequestVariantSpec {
    pub(crate) variant_name: Ident,
    pub(crate) payload_type: Type,
    pub(crate) opens: Option<Ident>,
}

#[derive(Clone)]
pub(crate) struct ReplyVariantSpec {
    pub(crate) variant_name: Ident,
    pub(crate) payload_type: Type,
}

#[derive(Clone)]
pub(crate) struct EventVariantSpec {
    pub(crate) variant_name: Ident,
    pub(crate) payload_type: Type,
    pub(crate) belongs: Option<Ident>,
}

/// Opt-in observer-subscription declaration. When present, the macro
/// injects the standardized `Tap(<Filter>) opens ObserverStream` /
/// `Untap(ObserverSubscriptionToken)` operations, an
/// `ObserverStream` (whose events carry the contract-author-supplied
/// event payload types), and an observer set / publish surface on
/// the daemon side.
///
/// Per /246 §2 (psyche-affirmed 2026-05-20T02:00Z), the observability
/// open/close verbs are fixed at `Tap` / `Untap` so `persona-introspect`
/// sees a uniform vocabulary across every observable channel. Domain
/// contracts that want the verb `Tap` rename their domain verb, not
/// the observability verb.
///
/// The event grammar splits `operation_event <Type>;` from
/// `effect_event <Type>;` so the macro knows which event record maps
/// to which publication moment (`publish_operation_received` vs
/// `publish_effect_emitted`).
#[derive(Clone)]
pub(crate) struct ObservableBlockSpec {
    /// Filter declaration. Either the macro-generated closed-enum
    /// default (`filter default;`) or a contract-author-named type
    /// (`filter <Type>;`) for which the contract crate provides the
    /// `ObserverFilterMatch` impl.
    pub(crate) filter: FilterDecl,
    /// Contract-author-defined event record type that names the
    /// `OperationReceived` publication moment (executor pre-lowering).
    /// Exactly one per observable block.
    pub(crate) operation_event: Ident,
    /// Contract-author-defined event record type that names the
    /// `EffectEmitted` publication moment (executor post-commit).
    /// Exactly one per observable block.
    pub(crate) effect_event: Ident,
}

/// How the observable block declares its filter type.
///
/// `Default` — macro generates `ObserverFilter` as a closed
/// enum (`All | OperationsOnly | EffectsOnly`) plus the matching
/// `ObserverFilterMatch` impl. Per /246 §4.
///
/// `Named` — contract author supplies the filter type and writes the
/// `ObserverFilterMatch` impl against it.
#[derive(Clone)]
pub(crate) enum FilterDecl {
    Default,
    Named(Ident),
}

#[derive(Clone)]
pub(crate) struct SchemaSpec {
    pub(crate) definitions: BTreeMap<String, SchemaDefinition>,
}

#[derive(Clone)]
pub(crate) struct SchemaDefinition {
    pub(crate) name: String,
    pub(crate) variants: Vec<SchemaVariant>,
    pub(crate) alias: Option<SchemaType>,
}

#[derive(Clone)]
pub(crate) enum SchemaVariant {
    Unit {
        name: String,
    },
    Data {
        name: String,
        fields: Vec<SchemaField>,
    },
}

#[derive(Clone)]
pub(crate) struct SchemaField {
    pub(crate) name: Option<String>,
    pub(crate) schema_type: SchemaType,
}

#[derive(Clone)]
pub(crate) enum SchemaType {
    Named(String),
    Vec(Box<SchemaType>),
    Option(Box<SchemaType>),
}

impl SchemaField {
    pub(crate) fn inferred(schema_type: SchemaType) -> Self {
        Self {
            name: None,
            schema_type,
        }
    }

    pub(crate) fn named(name: impl Into<String>, schema_type: SchemaType) -> Self {
        Self {
            name: Some(name.into()),
            schema_type,
        }
    }
}

impl ObservableBlockSpec {
    /// Enumerate every event record this block declares. Used by the
    /// emit pass when synthesising the channel's event enum and the
    /// `ObserverStream` block.
    pub(crate) fn event_records(&self) -> [&Ident; 2] {
        [&self.operation_event, &self.effect_event]
    }
}

impl ChannelSpec {
    pub(crate) fn is_streaming(&self) -> bool {
        self.event.is_some() || !self.streams.is_empty() || self.observable.is_some()
    }
}
