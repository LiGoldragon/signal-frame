# signal-frame skill

Work here only on shared Signal wire-kernel records and frame mechanics.

- Keep this crate domain-free. Domain records live in per-component
  contract crates (`signal-<component>`).
- Keep this crate verb-free. The six Sema verbs (`Assert`, `Mutate`,
  `Retract`, `Match`, `Subscribe`, `Validate`) live in `signal-sema`,
  not here. `Operation<Payload>` and `Request<Payload>` carry payloads
  directly without a universal verb tag.
- Keep runtime code out: no actors, tokio loops, redb stores, terminal
  adapters, or CLI parsing.
- Add tests that round-trip real typed frames through rkyv. Do not
  prove behavior by grepping strings.
- Use full English identifiers and keep reusable behavior on
  data-bearing types.
- The `signal_channel!` macro lives in the sibling `signal-frame-macros`
  proc-macro crate and is re-exported from this crate's `lib.rs`. The
  macro currently retains its pre-migration verb-tagged input shape —
  see `macros/README.md` for the redesign that lifts contract-local
  operations into the macro grammar.
