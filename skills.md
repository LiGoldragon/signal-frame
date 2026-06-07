# signal-frame skill

Work here only on shared Signal wire-kernel records and frame mechanics.

- Keep this crate domain-free. Domain records live in per-component
  contract crates (`signal-<component>`).
- Keep this crate verb-free. The six Sema verbs (`Assert`, `Mutate`,
  `Retract`, `Match`, `Subscribe`, `Validate`) live in `signal-sema`,
  not here. `Request<Payload>` carries payloads directly — each payload
  is itself a contract operation whose NOTA record head names the
  contract-local verb. There is no per-operation kernel wrapper.
- Keep daemon/runtime code out: no actors, tokio loops, redb stores, or
  terminal adapters. Shared thin-CLI frame machinery belongs here when
  it is domain-free: single-argument parsing, ordinary-vs-meta socket
  routing, caller capture, frame send/receive, and NOTA reply rendering.
- Add tests that round-trip real typed frames through rkyv. Do not
  prove behavior by grepping strings.
- Use full English identifiers and keep reusable behavior on
  data-bearing types.
- Schema-driven Rust generation lives in `schema-rust-next` build
  generation, not in `signal-frame`. This crate re-exports
  `signal_channel!` for current hand-written contracts while those
  contracts migrate.
