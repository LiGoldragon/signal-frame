# signal-frame

Signal frame mechanics: the wire kernel for typed inter-component
communication in the primary workspace.

`signal-frame` owns the universal request/reply spine, protocol
version records, length-prefixed rkyv frame helpers, exchange
identifiers, async correlation primitives, stream / subscription
lifecycle, reply plumbing, and the `signal_channel!` declaration
macro. Domain records and contract-local operation roots live in
the per-component contract crates that depend on this one.

`signal-frame` is the renamed successor to the former `signal-core`
crate. The six Sema verbs (`Assert`, `Mutate`, `Retract`, `Match`,
`Subscribe`, `Validate`) — which lived in `signal-core` — have moved
to the sibling crate `signal-sema`, where they describe the
internal Sema-engine execution vocabulary rather than public
wire-contract verbs. See
`primary/reports/designer/238-signal-architecture-redirection-contract-local-verbs.md`
and `primary/reports/designer/239-signal-architecture-migration-plan.md`
in the workspace for the full architectural redirection.
