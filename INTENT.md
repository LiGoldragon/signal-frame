# INTENT - signal-frame

`signal-frame` is the shared Rust-to-Rust wire kernel for typed
component signals.

It owns domain-free frame mechanics: short headers, exchange and stream
identifiers, request/reply envelopes, streaming frame bodies, subscription
token inner values, observable-set traits, length-prefixed rkyv archive
helpers, caller context, thin client-side frame plumbing, and the
`nota-next` text projection for its own frame-kernel records.

It does not own component domain records, daemon runtime loops, Nexus
decisions, SEMA storage, policy authority, or universal Sema verbs. Those
belong to generated component contracts, `triad-runtime`, `sema-engine`, or
the specific component daemon.

Streaming push uses this crate as the low-level wire kernel only.
`StreamingFrameBody::SubscriptionEvent` and `SubscriptionTokenInner` are the
binary transport shape; schema declares which component operations open a
stream and which event variants belong to it; `triad-runtime` owns reusable
token issuance, registries, and event-frame publication. Component daemons
supply stream filters and delivery IO.

The ordinary/meta split is a contract/rebuild boundary, not an engine split:
working components depend on `signal-<component>` for ordinary calls, and
security-sensitive or policy-editing callers depend on `meta-signal-<component>`.
Both surfaces still use this same frame kernel.
