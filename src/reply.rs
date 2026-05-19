//! Reply: the typed response shape for a frame-layer exchange.
//!
//! Pre-execution rejection and in-execution abort are different
//! categories: one has no per-op results because no op ran; the other
//! has per-op results because some did. Splitting the variants makes
//! illegal states unrepresentable.
//!
//! Under the contract-local-verb architecture (see
//! `primary/reports/designer/238` and `/239`), per-op replies no longer
//! carry a universal `SignalVerb` tag. The per-op reply is
//! positionally addressed — its index in the `per_operation`
//! [`NonEmpty`] matches the index in the originating request's
//! operation sequence.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use thiserror::Error;

use crate::non_empty::NonEmpty;

/// Reply is a typed sum. Pre-execution rejection and in-execution
/// abort are different categories; splitting the variants makes
/// illegal states unrepresentable.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum Reply<ReplyPayload> {
    /// Request was accepted for execution. Per-op results follow.
    /// `outcome` distinguishes all-committed from aborted-at-N.
    Accepted {
        outcome: AcceptedOutcome,
        per_operation: NonEmpty<SubReply<ReplyPayload>>,
    },
    /// Request was rejected before any op began (pre-flight rule
    /// violation: decode error, malformed shape, daemon-specific
    /// pre-execution policy). No per-op results because no op ran.
    Rejected { reason: RequestRejectionReason },
}

impl<ReplyPayload> Reply<ReplyPayload> {
    pub fn completed(per_operation: NonEmpty<SubReply<ReplyPayload>>) -> Self {
        Self::Accepted {
            outcome: AcceptedOutcome::Completed,
            per_operation,
        }
    }

    pub fn aborted(
        failed_at: usize,
        reason: OperationFailureReason,
        per_operation: NonEmpty<SubReply<ReplyPayload>>,
    ) -> Self {
        Self::Accepted {
            outcome: AcceptedOutcome::Aborted { failed_at, reason },
            per_operation,
        }
    }

    pub fn rejected(reason: RequestRejectionReason) -> Self {
        Self::Rejected { reason }
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum AcceptedOutcome {
    /// Every operation completed/committed in its own mode.
    Completed,
    /// An operation at `failed_at` failed; the request aborted.
    Aborted {
        failed_at: usize,
        reason: OperationFailureReason,
    },
}

/// Why a request was rejected before any operation ran. Frame-layer
/// rkyv decode failures are protocol errors (close + log), not typed
/// rejections — they have no `ExchangeIdentifier` to address.
///
/// Under the contract-local-verb architecture the former universal
/// rejection reasons (verb/payload mismatch, Subscribe-out-of-position)
/// no longer apply. Daemon-specific pre-execution policies surface as
/// `Internal` or as channel-defined reply variants.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RequestRejectionReason {
    /// Receiver-internal error before any op ran.
    #[error("receiver-internal pre-execution error")]
    Internal,
}

/// Why an operation failed during execution.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum OperationFailureReason {
    /// A pre-condition (expected revision, etc.) was not satisfied.
    #[error("pre-condition failed")]
    PreconditionFailed,
    /// A validation predicate failed at the daemon layer.
    #[error("validate predicate failed")]
    ValidationFailed,
    /// The domain receiver rejected the operation.
    #[error("domain receiver rejected the operation")]
    DomainRejection,
}

/// Per-operation reply variant. Each variant carries only the fields a
/// reader needs, so illegal combinations (Ok with no payload, Skipped
/// with payload) are unrepresentable.
///
/// Per-op replies are positionally addressed — the index in the
/// `per_operation` [`NonEmpty`] aligns with the originating request's
/// operation index. No universal verb tag is carried.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum SubReply<ReplyPayload> {
    /// Op ran and committed/completed; only emitted under
    /// [`AcceptedOutcome::Completed`].
    Ok { payload: ReplyPayload },
    /// Op ran but its result is no longer authoritative because the
    /// request as a whole aborted.
    Invalidated,
    /// Op was attempted and failed; this is the cause of the abort.
    /// Exactly one per [`AcceptedOutcome::Aborted`] reply, at
    /// `failed_at`.
    Failed {
        reason: OperationFailureReason,
        detail: Option<ReplyPayload>,
    },
    /// Op never ran because an earlier op failed.
    Skipped,
}
