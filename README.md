# signal-frame

Signal frame mechanics: the wire kernel for typed inter-component
communication in the primary workspace.

`signal-frame` owns the universal request/reply spine, protocol
version records, length-prefixed rkyv frame helpers, exchange
identifiers, async correlation primitives, stream / subscription
lifecycle, reply plumbing, and `dotos` projection for its own
frame-kernel records. It also owns the shared `signal_cli!` thin-client skeleton
used by component CLIs: one DOTOS argument in, one typed frame to the
daemon, one DOTOS reply out. Domain records and contract-local operation
roots live in the per-component contract crates that depend on this
one.

Every frame uses the contract-bound envelope seam. Its
eight-byte short header packs `ContractId(u32)`, `WireRevision(u16)`,
`VariantCode(u8)`, and `RootCode(u8)` in that bit order. Contract crates
implement `WireContract`; bound constructors derive the binding for
request, reply, handshake/control, and subscription-event frames, and
bound decoders reject zero, wrong-contract, and wrong-revision
headers before archive decoding. Allocation constants and registry
enumerations do not live in this generic crate.

## 0.4 migration

The producer API is deliberately breaking. `LegacyExchangeFrame`,
`LegacyStreamingFrame`, and unchecked `ShortHeader` constructors are
removed. Consumers must implement `WireContract` and use
`BoundExchangeFrame` or `BoundStreamingFrame`. Prefix peeking now returns
`RawShortHeader`; call `validate` before using it as a `ShortHeader`.
`Request::route` and macro-generated `Operation::into_frame` are fallible
because route values with bits above the low sixteen are rejected.

Generated operation handling has one wire ingress: `OperationDispatch::dispatch`
accepts the bound frame, validates its binding and complete route, and checks
archive/body header equality. Only then does it invoke the handler with the
private-constructor `ValidatedOperation` capability. Handlers can inspect or
consume that trusted typed operation through its accessors; arbitrary decoded
operations cannot call the handler directly.

`signal-frame` is the renamed successor to the former `signal-core`
crate. The six Sema verbs (`Assert`, `Mutate`, `Retract`, `Match`,
`Subscribe`, `Validate`) — which lived in `signal-core` — have moved
to the sibling crate `signal-sema`, where they describe the
internal Sema-engine execution vocabulary rather than public
wire-contract verbs.

Schema-driven Rust generation lives in `schema-rust` build
generation, not in a `signal-frame` proc macro. The frame kernel is a
dependency of generated contracts; it is not the schema emitter.

During migration, `signal_channel!` keeps the hand-written contract
grammar alive for contracts that have not moved to schema:

```rust
signal_channel! {
    channel Message contract MessageContract {
        operation Submit(Submission),
        operation Query(InboxQuery),
    }
    reply MessageReply {
        Accepted(Receipt),
        Inbox(Inbox),
    }
}
```

The `signal_cli!` macro emits a complete thin CLI when the working and
meta contracts follow the component naming convention:

```rust
signal_frame::signal_cli!(spirit, signal_spirit);
```

Generated CLIs enforce the single-argument rule, route request heads to
the working or meta socket, inject advisory parent-process `Caller`
context into the binary frame, and render accepted typed replies back to
DOTOS. Socket credentials and policy checks remain daemon concerns.
