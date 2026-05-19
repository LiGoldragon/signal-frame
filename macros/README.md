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
        operation Observe(Push),
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

## Validation witnesses

- `tests/channel_macro.rs` proves a positive non-streaming channel,
  a streaming channel, generated kind methods, frame aliases, and
  NOTA round trips.
- `tests/ui/channel_macro/` carries compile-fail witnesses for the
  retired verb-tagged grammar and the retained structural checks:
  duplicate record heads, orphan streams, reverse event belongs
  mismatch, and close-token type mismatch.

The macro's responsibility is unchanged at the level of intent: take
one channel declaration and emit the typed request/reply/event
vocabulary, frame aliases, stream-relation witnesses, and the NOTA
codec on the payload layer. Only the input grammar and the
verb-tagging output change.
