# signal-frame-macros

Proc-macro engine for `signal_channel!`.

## MUST IMPLEMENT — signal architecture migration (`/239 §3.B`)

**This crate carries the pre-migration verb-tagged macro shape.** It
was copied unchanged from the former `signal-core/macros/` as part of
the initial `signal-frame` scaffold. The full redesign to
contract-local verbs is **deferred to a follow-up arc** — but a
future agent picking up macro work must finish the redesign before
any new contract crate can build cleanly against `signal-frame`.

### Where the macro stands today

- The macro's input grammar still parses the **verb-tagged shape**:

  ```rust
  signal_channel! {
      channel Ledger {
          request Request {
              Assert ReceiveHookNotification(ReceiveHookNotification),
              Match RecentRepositoriesQuery(RecentRepositoriesQuery),
              // ...
          }
      }
  }
  ```

- The emitted code still references `::signal_frame::SignalVerb` and
  `RequestPayload::signal_verb()`. **Neither exists in
  `signal-frame`** — the migration removed both.

- Result: the proc-macro crate itself compiles (it's pure token
  generation), but any `signal_channel!` invocation against
  `signal-frame` produces code that does not compile. The macro's
  validate/parse compile-fail tests still work because they error
  before reaching the emit stage.

### What the redesign must produce

The new input grammar per `/239 §3.B`:

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

The macro emits the same outputs (`Operation` enum, `Request`
wrapper, `Frame` alias, `RequestBuilder`, NOTA codecs) **minus** the
verb-tagging machinery:

- No `SignalVerb` references — the verb spine is gone at this layer.
- No `signal_verb()` method on `RequestPayload`.
- No `verb` field on `Operation<Payload>`.
- No verb-membership check or `Subscribe`-position rule in
  `validate.rs`. (Subscribe is now a contract-local verb, not a
  universal one.)
- The macro's `validate.rs` checks on variant uniqueness, projected
  NOTA record-head uniqueness, and stream-block cross-references
  carry over unchanged.

### Concrete redesign tasks

1. **Update `parse.rs`** so each request variant parses as
   `operation <Verb>(<Payload>)` — no leading verb keyword from the
   six-Sema-verbs set. The `verb_keyword` field on
   `RequestVariantSpec` goes away (or becomes the variant identifier
   directly).
2. **Update `emit.rs`** to emit the contract-local-verb shape: drop
   the `RequestPayload::signal_verb()` impl entirely; drop the
   `verb:` field handling; keep the NOTA codec that maps each
   variant name to its record head.
3. **Update `validate.rs`**: remove `validate_verbs` (no universal
   verb set); keep the variant-uniqueness, record-head, and stream
   relation checks. The current rule "close variant must be tagged
   `Retract`" goes away — close is a contract-local concept now.
4. **Update `model.rs`**: drop `SIGNAL_VERBS` and `verb_keyword`.
5. **Update or retire the compile-fail tests** in
   `signal-frame/tests/ui/channel_macro/`: `unknown_verb.rs` and
   `non_subscribe_opens.rs` lose their meaning under the new
   grammar; replace with negative tests of the new shape.
6. **Restore the positive macro test** (`tests/channel_macro.rs` —
   currently held out; see the parent `tests/` directory).

### References

- `primary/reports/designer/238-signal-architecture-redirection-contract-local-verbs.md`
- `primary/reports/designer/239-signal-architecture-migration-plan.md` §3.B
- `signal-frame/macros/src/lib.rs` — MUST IMPLEMENT comment block
- `signal-frame/AGENTS.md` — workspace contract for this repo

The macro's responsibility is unchanged at the level of intent: take
one channel declaration and emit the typed request/reply/event
vocabulary, frame aliases, stream-relation witnesses, and the NOTA
codec on the payload layer. Only the input grammar and the
verb-tagging output change.
