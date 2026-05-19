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
wire-contract verbs.

The `signal_channel!` macro declares contract-local operation roots
directly:

```rust
signal_channel! {
    channel Message {
        operation Submit(Submission),
        operation Query(InboxQuery),
    }
    reply MessageReply {
        Accepted(Receipt),
        Inbox(Inbox),
    }
}
```
