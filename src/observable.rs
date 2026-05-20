//! [`ObservableSet`] and [`ObservationProjection`]: shape contracts
//! for the observable-channel publish pipeline.
//!
//! The macro emits a `<Channel>ObserverSet` that owns subscription
//! bookkeeping (register/unregister/publish). Downstream bridge code,
//! daemon glue, and tests need to compose with these sets generically
//! without coupling to a specific channel. The bridge also needs a
//! projection step from raw execution facts (`Operation`, `Effect`) to
//! the channel's event records (`OperationEvent`, `EffectEvent`).
//!
//! Both traits live in signal-frame for two reasons:
//!
//! 1. The macro that emits the per-channel impl and alias also lives
//!    in signal-frame; the macro can reference these traits unqualified.
//! 2. signal-executor depends on signal-frame, not the reverse; a
//!    trait in signal-frame is reachable from the bridge in
//!    signal-executor.
//!
//! Neither trait mentions Sema-layer types. `ObservationProjection`'s
//! `Effect` associated type is the execution-fact type, generic at the
//! trait level. A downstream executor bridge constrains it to that
//! executor's own effect type at the impl site. This keeps the
//! projection trait pure and avoids any dependency from `signal-frame`
//! back into an executor crate.

/// Shape contract for macro-generated per-channel observer sets.
///
/// Implementors deliver a published event to every subscribed observer
/// whose filter accepts it. The `deliver` callback runs once per
/// matching subscription, in registration order, receiving the
/// subscription token and a reference to the event.
///
/// `OperationEvent` and `EffectEvent` correspond to the
/// `operation_event` / `effect_event` declarations in the channel's
/// `observable` block:
///
/// * `publish_operation_received` -- called by the bridge after the
///   executor publishes that an inbound contract operation arrived
///   (before lowering).
/// * `publish_effect_emitted` -- called by the bridge after the
///   executor publishes that an effect committed (after atomic
///   commit).
pub trait ObservableSet {
    /// Token type the observer set issues from `register` and accepts
    /// to `unregister`. Macro-generated as
    /// `<Channel>ObserverSubscriptionToken`.
    type Token: Copy;

    /// Record type for the `operation_event` declaration in the
    /// channel's `observable` block.
    type OperationEvent;

    /// Record type for the `effect_event` declaration in the channel's
    /// `observable` block.
    type EffectEvent;

    /// Publish that an inbound contract operation was received. The
    /// `deliver` closure runs once per matching subscription, in
    /// registration order.
    fn publish_operation_received<DeliverObserver>(
        &self,
        event: &Self::OperationEvent,
        deliver: DeliverObserver,
    ) where
        DeliverObserver: FnMut(Self::Token, &Self::OperationEvent);

    /// Publish that an effect committed. The `deliver` closure runs
    /// once per matching subscription, in registration order.
    fn publish_effect_emitted<DeliverObserver>(
        &self,
        event: &Self::EffectEvent,
        deliver: DeliverObserver,
    ) where
        DeliverObserver: FnMut(Self::Token, &Self::EffectEvent);
}

/// Crate-boundary projection from raw execution facts to channel
/// event records.
///
/// The bridge between an executor (raw `Operation` and `Effect`
/// values) and an observer set (per-channel `OperationEvent` and
/// `EffectEvent` records) needs an explicit projection step. This
/// trait names that projection.
///
/// `Operation` is the contract's operation enum (e.g.
/// `LedgerOperation`); `Effect` is the executor's execution-fact
/// type (e.g. `signal_executor::SemaEffect`), kept generic so this
/// trait does not depend on signal-executor's Sema types. The
/// daemon's projection impl typically constrains the associated types
/// via the macro-generated `<Channel>ObservationProjection` trait
/// alias, which pins `Operation`, `OperationEvent`, and `EffectEvent`
/// to the contract's declarations.
///
/// A daemon that does not observe simply does not implement this
/// trait; it pays no associated-type cost on its `Lowering` impl.
pub trait ObservationProjection {
    /// Contract's operation enum.
    type Operation;

    /// Execution-fact effect type. Bridge implementations constrain
    /// this to the executor's `SemaEffect` at the use site.
    type Effect;

    /// Record type for the channel's `operation_event` declaration.
    type OperationEvent;

    /// Record type for the channel's `effect_event` declaration.
    type EffectEvent;

    /// Project a raw contract operation into the
    /// `operation_event` record the channel publishes before lowering.
    fn operation_event(&self, operation: &Self::Operation) -> Self::OperationEvent;

    /// Project a raw effect into the `effect_event` record the
    /// channel publishes after atomic commit.
    fn effect_event(&self, effect: &Self::Effect) -> Self::EffectEvent;
}
