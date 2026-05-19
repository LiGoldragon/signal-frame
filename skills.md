# signal-frame skill

Work here only on shared Signal wire-kernel records and frame mechanics.

- Keep this crate domain-free. Domain records live in per-component
  contract crates (`signal-<component>`).
- Keep this crate verb-free. The six Sema verbs (`Assert`, `Mutate`,
  `Retract`, `Match`, `Subscribe`, `Validate`) live in `signal-sema`,
  not here. `Request<Payload>` carries payloads directly — each payload
  is itself a contract operation whose NOTA record head names the
  contract-local verb. There is no per-operation kernel wrapper.
- Keep runtime code out: no actors, tokio loops, redb stores, terminal
  adapters, or CLI parsing.
- Add tests that round-trip real typed frames through rkyv. Do not
  prove behavior by grepping strings.
- Use full English identifiers and keep reusable behavior on
  data-bearing types.
- The `signal_channel!` macro lives in the sibling `signal-frame-macros`
  proc-macro crate and is re-exported from this crate's `lib.rs`. It
  declares contract-local operations directly:
  `operation Submit(Submission)`, not `Assert Submit(Submission)`.
