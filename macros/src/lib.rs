//! `signal_channel!` proc-macro — a small contract compiler.
//!
//! ```ignore
//! signal_channel! {
//!     channel Ledger {
//!         operation Receive(HookNotification),
//!         operation Push(Push),
//!         operation Query(Query),
//!         // ...
//!     }
//!     reply LedgerReply {
//!         Received(ReceivedAcknowledgement),
//!         Observed(ObservationAcknowledgement),
//!         QueryResult(QueryResult),
//!     }
//! }
//! ```
//!
//! The macro emits the same frame-kernel outputs without any
//! universal verb tag: no `SignalVerb`, no `signal_verb()` method,
//! and no per-operation kernel wrapper. Contract-local operation
//! roots are the generated request-payload enum variants.
//!
//! The macro reads one typed channel declaration and emits the
//! request/reply/event payload enums, kind enums, frame aliases,
//! stream-relation witnesses, and the NOTA codec on the payload layer.
//!
//! Channels can opt into observation by declaring an `observable`
//! block; the macro then injects contract-author-named open/close
//! operations, an `ObserverStream`, an `ObserverSubscriptionOpened`
//! reply variant, and a per-channel `ObserverSet` runtime with
//! `publish_*` methods.
//! See `macros/README.md` for the observable grammar.
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
