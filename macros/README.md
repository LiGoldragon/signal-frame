# signal-frame-macros

Proc-macro front door for production `signal_channel!` declarations.

Schema-driven Rust generation lives in `schema-rust` build
generation. This proc-macro crate keeps the current hand-written
contract declaration grammar alive while contracts migrate.

Every declaration names a `WireContract` marker. The macro emits bound frame
aliases, so callers cannot select a contract or revision per frame.

## Contract-Local Operation Grammar

The macro's input grammar declares contract-local operation roots
directly. The operation root is the caller's domain action; no
universal Sema verb appears at this layer.

```rust
signal_channel! {
    channel Ledger contract LedgerWire {
        operation Receive(HookNotification),
        operation Push(Push),
        operation Query(Query),
        // ...
    }
    reply Reply {
        Received(ReceivedAcknowledgement),
        Pushed(PushAcknowledgement),
        QueryResult(QueryResult),
    }
}
```

The macro emits the same outputs (contract-local operation enum,
`Frame` alias, `Request` / `RequestBuilder` aliases over the payload
enum, reply conversion impls, kind enums, DOTOS codecs) without any
verb-tagging machinery. Emitted names are intentionally unprefixed:
`Operation`, `Reply`, `Event`, `Frame`, `FrameBody`, `Request`,
`ReplyEnvelope`, and `RequestBuilder`. If a crate declares multiple
channels, it puts each invocation in its own Rust module and uses the
module path as the namespace.

- No `SignalVerb` references — the verb spine is gone at this layer.
- No `signal_verb()` method on `RequestPayload`.
- No per-operation kernel wrapper — `Request<Payload>` carries
  `NonEmpty<Payload>` directly. Per-op metadata, if a contract ever
  needs it, goes in the payload type.
- No hand-written `OperationKind` enum or `From<Payload> for Reply`
  stack — both are structurally derived from the declaration.
- No verb-membership check or stream-opening rule tied to a specific
  verb name. `Subscribe`, when a contract uses that word, is just a
  contract-local operation like any other.
- The macro's `validate.rs` checks on variant uniqueness, projected
  DOTOS record-head uniqueness, and stream-block cross-references
  carry over unchanged.

## Optional `observable` Block

A channel can opt into an observer-subscription surface by declaring
an `observable` block. When present the macro injects the
standardized `Tap(<Filter>) opens ObserverStream` /
`Untap(<Token>)` operations (mandatory, no author override), an
`ObserverStream` whose token type is auto-generated, a reply variant
`ObserverSubscriptionOpened`, and a runtime `ObserverSet` with
`publish_*` methods the daemon's executor calls.

```rust
signal_channel! {
    channel Spirit contract SpiritWire {
        operation State(Statement),
        operation Record(Entry),
    }
    reply Reply { … }
    observable {
        filter default;
        operation_event OperationReceived;
        effect_event EffectEmitted;
    }
}
```

The observability verbs `Tap` / `Untap` are macro-mandated per
`reports/designer/246-v4-bundled-fix-deep-design-with-examples.md` §2
so `persona-introspect` sees a uniform vocabulary across every
observable channel. A contract that legitimately wants `Tap` (or
`Untap`) as a domain verb renames its domain verb — the
observability verbs are not negotiable.

The `filter` declaration takes either a contract-author-named type
(`filter <Type>;`, in which case the contract crate writes the
`ObserverFilterMatch` impl) or the macro-generated default
(`filter default;`), which produces a closed-enum filter with
`All` / `OperationsOnly` / `EffectsOnly` variants and the matching
trait impl. Use `filter default;` when role-based filtering suffices;
use `filter <Type>;` when subscribers need richer predicates.

The `operation_event` and `effect_event` idents name event record
types the contract crate declares. They map to the two fixed
publication moments: `matches_operation_received` /
`publish_operation_received` (before lowering) and
`matches_effect_emitted` / `publish_effect_emitted` (after atomic
commit).

Per-event names are workspace-uniform vocabulary
(`OperationReceived`, `EffectEmitted`) so cross-component
observers — persona-introspect, debug tooling — subscribe to the
same record-head language across every observable channel. Rust-side
observable helper names are unprefixed (`ObserverSubscriptionToken`,
`ObserverSubscriptionOpened`, `ObserverSet`, `ObserverFilterMatch`);
multiple observable channels coexist by living in separate modules.

The publish methods take a delivery closure
(`FnMut(ObserverSubscriptionToken, &Event)`) — the macro
filters and routes, the executor / daemon dispatches the event onto
the matching observers' subscription streams.

The observable block is opt-in: channels without it produce no
observer surface. (Persona components are expected to declare it;
small leaf utilities may omit it.)

## Validation witnesses

- `tests/channel_macro.rs` proves a positive non-streaming channel,
  a streaming channel, generated kind methods, frame aliases, DOTOS
  round trips, and the observable surface: macro-mandated `Tap` /
  `Untap` injection, stream witnesses, reply variant round-trips,
  the observer-set's filter-routing behaviour, and the
  `filter default;` closed-enum generation.
- `tests/ui/channel_macro/` carries compile-fail witnesses for the
  retired verb-tagged grammar, the retained structural checks
  (duplicate record heads, orphan streams, reverse event belongs
  mismatch, close-token type mismatch), and the observable block's
  failure modes (missing contract marker, missing filter, missing events, domain operation
  named `Tap` or `Untap`, duplicate block).

The macro's responsibility is unchanged at the level of intent: take
one channel declaration and emit the typed request/reply/event
vocabulary, frame aliases, stream-relation witnesses, and the DOTOS
codec on the payload layer. Only the input grammar and the
verb-tagging output change.
