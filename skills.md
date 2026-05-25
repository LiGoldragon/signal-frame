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
  it is domain-free: single-argument parsing, working-vs-owner socket
  routing, caller capture, frame send/receive, and NOTA reply rendering.
- Add tests that round-trip real typed frames through rkyv. Do not
  prove behavior by grepping strings.
- Use full English identifiers and keep reusable behavior on
  data-bearing types.
- The `emit_schema!` macro lives in the sibling `signal-frame-macros`
  proc-macro crate and is re-exported from this crate's `lib.rs`. It is
  the schema-driven Rust composer entrypoint. The old handwritten macro
  body is `legacy_signal_channel!` during the migration window;
  `signal_channel!` is only a compatibility alias while existing
  contracts move.
- Schema-generated modules now include the first crystallized
  architecture surfaces: prefix-preserving `ExtendedHeader`,
  route-derived `Effect` vocabulary, `EffectTable`, `Interact`,
  `InteractionActor`, and fan-out output scaffolding. Tests should
  instantiate these generated types, not merely grep emitted tokens.
