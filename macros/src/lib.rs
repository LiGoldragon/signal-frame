//! `signal_channel!` proc-macro — a small contract compiler.
//!
//! ============================================================
//! MUST IMPLEMENT — signal architecture migration (`/239` §3.B)
//! ============================================================
//!
//! The macro in this crate was copied from the former
//! `signal-core/macros/` unchanged. Its **input grammar still expects
//! the pre-migration verb-tagged shape** —
//!
//! ```ignore
//! signal_channel! {
//!     channel Ledger {
//!         request Request {
//!             Assert ReceiveHookNotification(ReceiveHookNotification),
//!             Match RecentRepositoriesQuery(RecentRepositoriesQuery),
//!             // ...
//!         }
//!     }
//! }
//! ```
//!
//! — and the **emitted code still references `SignalVerb` and
//! `signal_verb()`**, neither of which exists in `signal-frame`.
//! That means: while this proc-macro crate compiles on its own,
//! any `signal_channel!` invocation against `signal-frame` will
//! produce code that does not compile.
//!
//! **The full redesign is deferred to a follow-up arc.** A future
//! agent picking up this work must change the input grammar and the
//! emitted code to the contract-local-verb shape:
//!
//! ```ignore
//! signal_channel! {
//!     channel Ledger {
//!         operation Receive(HookNotification),
//!         operation Observe(Push),
//!         operation Query(Query),
//!         // ...
//!     }
//! }
//! ```
//!
//! The macro then emits the same outputs (`Operation` enum,
//! `Request` wrapper, `Frame`, `RequestBuilder`, codec impls) **minus
//! the verb-tagging machinery**: no `SignalVerb` references, no
//! `signal_verb()` method, no `Operation::verb` field. The macro's
//! `validate.rs` checks (variant uniqueness, record-head uniqueness,
//! stream-block cross-references) carry over; the verb-membership
//! check and Subscribe-position rule go away.
//!
//! References:
//! - `primary/reports/designer/238-signal-architecture-redirection-contract-local-verbs.md`
//! - `primary/reports/designer/239-signal-architecture-migration-plan.md` §3.B
//! - `signal-frame/macros/README.md` for a complete redesign note.
//!
//! ============================================================
//! End MUST IMPLEMENT block.
//! ============================================================
//!
//! The macro reads one typed channel declaration and emits the
//! request/reply/event payload enums, kind enums, frame aliases,
//! stream-relation witnesses, and the NOTA codec on the payload layer.
//!
//! Does not emit actors, sockets, storage, routing, policy closures,
//! daemon code, or hidden runtime behaviour.

mod emit;
mod model;
mod parse;
mod validate;

use proc_macro::TokenStream;
use syn::parse_macro_input;

use crate::model::ChannelSpec;

#[proc_macro]
pub fn signal_channel(input: TokenStream) -> TokenStream {
    let spec = parse_macro_input!(input as ChannelSpec);
    if let Err(error) = validate::validate(&spec) {
        return error.into_compile_error().into();
    }
    emit::emit(&spec).into()
}
