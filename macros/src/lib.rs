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
//!     reply Reply {
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
//! reply conversion impls, stream-relation witnesses, and the NOTA
//! codec on the payload layer. Emitted names are clean and
//! unprefixed; crates with multiple channels use Rust modules for
//! disambiguation.
//!
//! Channels can opt into observation by declaring an `observable`
//! block; the macro then injects standardized `Tap`/`Untap`
//! operations, an `ObserverStream`, an `ObserverSubscriptionOpened`
//! reply variant, and an `ObserverSet` runtime with
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
