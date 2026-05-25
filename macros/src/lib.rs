//! Signal proc macros.
//!
//! ```ignore
//! legacy_signal_channel! {
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
//! `legacy_signal_channel!` emits the frame-kernel outputs for the old
//! handwritten macro body without any universal verb tag: no
//! `SignalVerb`, no `signal_verb()` method, and no per-operation
//! kernel wrapper. Contract-local operation roots are the generated
//! request-payload enum variants.
//!
//! `emit_schema!` is the new schema-driven Rust composer entrypoint.
//! It reads assembled schema data through the `schema` crate and must
//! not route through the legacy `ChannelSpec` parser/emitter.
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
mod schema_entry;
mod validate;

use proc_macro::TokenStream;

use crate::model::ChannelSpec;

#[proc_macro]
pub fn emit_schema(input: TokenStream) -> TokenStream {
    schema_entry::emit_schema(input)
}

#[proc_macro]
pub fn legacy_signal_channel(input: TokenStream) -> TokenStream {
    expand_legacy_signal_channel(input)
}

#[proc_macro]
pub fn signal_channel(input: TokenStream) -> TokenStream {
    expand_legacy_signal_channel(input)
}

fn expand_legacy_signal_channel(input: TokenStream) -> TokenStream {
    let spec = match syn::parse::<ChannelSpec>(input) {
        Ok(spec) => spec,
        Err(error) => return error.into_compile_error().into(),
    };
    if let Err(error) = validate::validate(&spec) {
        return error.into_compile_error().into();
    }
    emit::emit(&spec).into()
}
