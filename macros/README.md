# signal-frame-macros

Proc-macro engine for `signal_channel!`.

## Contract-local operation grammar

The macro's input grammar declares contract-local operation roots
directly. The operation root is the caller's domain action; no
universal Sema verb appears at this layer.

```rust
signal_channel! {
    channel Ledger {
        operation Receive(HookNotification),
        operation Push(Push),
        operation Query(Query),
        // ...
    }
    reply LedgerReply {
        Received(ReceivedAcknowledgement),
        Pushed(PushAcknowledgement),
        QueryResult(QueryResult),
    }
}
```

The macro emits the same outputs (contract-local operation enum,
`Frame` alias, `Request` / `RequestBuilder` aliases over the payload
enum, NOTA codecs) without any verb-tagging machinery:

- No `SignalVerb` references — the verb spine is gone at this layer.
- No `signal_verb()` method on `RequestPayload`.
- No per-operation kernel wrapper — `Request<Payload>` carries
  `NonEmpty<Payload>` directly. Per-op metadata, if a contract ever
  needs it, goes in the payload type.
- No verb-membership check or stream-opening rule tied to a specific
  verb name. `Subscribe`, when a contract uses that word, is just a
  contract-local operation like any other.
- The macro's `validate.rs` checks on variant uniqueness, projected
  NOTA record-head uniqueness, and stream-block cross-references
  carry over unchanged.

## Optional `observable` block

A channel can opt into an observer-subscription surface by declaring
an `observable` block. When present the macro injects two
contract-author-named operations, an `ObserverStream` whose token type
is auto-generated, a reply variant
`ObserverSubscriptionOpened`, and a runtime `<Channel>ObserverSet`
with `publish_*` methods the daemon's executor calls.

```rust
signal_channel! {
    channel Spirit {
        operation State(Statement),
        operation Record(Entry),
    }
    reply SpiritReply { … }
    observable {
        open Watch(ObserverFilter);
        close Unwatch;
        filter ObserverFilter;
        operation_event OperationReceived;
        effect_event SemaEffectEmitted;
    }
}
```

The `filter`, `operation_event`, and `effect_event` idents name types
the contract crate declares. The macro emits a
`<Channel>ObserverFilterMatch` trait the contract author implements on
the filter type; the role-named `matches_operation_received` and
`matches_effect_emitted` methods route deliveries.

Per-event names are workspace-uniform vocabulary
(`OperationReceived`, `SemaEffectEmitted`) so cross-component
observers — persona-introspect, debug tooling — subscribe to the
same record-head language across every observable channel. The
Rust-side types are channel-prefixed (`<Channel>ObserverSubscriptionToken`,
`<Channel>ObserverSubscriptionOpened`) so multiple observable
channels can coexist in the same scope.

The publish methods take a delivery closure
(`FnMut(<Channel>ObserverSubscriptionToken, &Event)`) — the macro
filters and routes, the executor / daemon dispatches the event onto
the matching observers' subscription streams.

The observable block is opt-in: channels without it produce no
observer surface and remain backward-compatible with the previous
shape.

## Validation witnesses

- `tests/channel_macro.rs` proves a positive non-streaming channel,
  a streaming channel, generated kind methods, frame aliases, NOTA
  round trips, and the observable surface: contract-authored open /
  close encoding, stream witnesses, reply variant round-trips, and
  the observer-set's filter-routing behaviour.
- `tests/ui/channel_macro/` carries compile-fail witnesses for the
  retired verb-tagged grammar, the retained structural checks
  (duplicate record heads, orphan streams, reverse event belongs
  mismatch, close-token type mismatch), and the observable block's
  failure modes (missing filter, missing events, operation-name
  collision, duplicate block).

The macro's responsibility is unchanged at the level of intent: take
one channel declaration and emit the typed request/reply/event
vocabulary, frame aliases, stream-relation witnesses, and the NOTA
codec on the payload layer. Only the input grammar and the
verb-tagging output change.
