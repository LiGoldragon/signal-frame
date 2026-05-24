//! Signal frame mechanics.
//!
//! `signal-frame` owns the universal request/reply spine, protocol
//! version records, length-prefixed rkyv frame helpers, exchange
//! identifiers, async correlation primitives, stream / subscription
//! lifecycle, and reply plumbing. Domain records and contract-local
//! operation roots live in the per-component contract crates that
//! depend on this one.
//!
//! The six Sema verbs (`Assert`, `Mutate`, `Retract`, `Match`,
//! `Subscribe`, `Validate`) live in the sibling crate `signal-sema`,
//! not here.
//!
//! The `signal_channel!` macro lives in the sibling
//! `signal-frame-macros` crate and is re-exported from this crate.
//! Consumers import it as `signal_frame::signal_channel`.
//!
//! `signal_cli!` emits thin component CLI binaries over the shared frame
//! client path.

pub mod caller;
pub mod command_line;
pub mod error;
pub mod exchange;
pub mod frame;
pub mod identity;
pub mod log_variant;
pub mod namespace;
pub mod non_empty;
pub mod observable;
pub mod reply;
pub mod request;
pub mod subscription;
pub mod version;

pub use caller::{Caller, ExecutablePath, ProcessIdentifier, ProcessStartTime};
pub use command_line::{
    ClientFrame, ClientShape, CommandLineDispatch, CommandLineError, CommandLineRouteError,
    CommandLineRouteTable, CommandLineSocket, CommandLineSockets, RequestHead, RequestInput,
    RequestText, SignalOperationHeads, SingleArgument, SingleArgumentError,
};
pub use error::FrameError;
pub use exchange::{
    ExchangeHandshake, ExchangeIdentifier, ExchangeLane, ExchangeMode, LaneSequence, SessionEpoch,
    StreamEventIdentifier,
};
pub use frame::{
    ExchangeFrame, ExchangeFrameBody, SHORT_HEADER_BYTE_COUNT, ShortHeader, StreamingFrame,
    StreamingFrameBody, short_header_from_archive, short_header_from_length_prefixed,
};
pub use identity::{Revision, Slot};
pub use log_variant::LogVariant;
pub use namespace::{NamespaceSection, SECTION_CUTOFF};
pub use non_empty::{NonEmpty, NonEmptyError};
pub use observable::{ObservableSet, ObservationProjection};
pub use reply::{
    AcceptedOutcome, BatchErrorClassification, BatchFailureReason, CommitStatus,
    OperationFailureReason, Reply, RequestRejectionReason, RetryClassification, SubReply,
};
pub use request::{Request, RequestBuilder, RequestBuilderError, RequestPayload};
pub use subscription::SubscriptionTokenInner;
pub use version::{
    HandshakeRejectionReason, HandshakeReply, HandshakeRequest, ProtocolVersion,
    SIGNAL_FRAME_PROTOCOL_VERSION,
};

pub use paste;
pub use signal_frame_macros::signal_channel;
