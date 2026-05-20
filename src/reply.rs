//! Reply: the typed response shape for a frame-layer exchange.
//!
//! Pre-execution rejection and in-execution abort are different
//! categories: one has no per-operation results because no operation ran;
//! the other has per-operation results because some did. Splitting the variants makes
//! illegal states unrepresentable.
//!
//! Under the contract-local-verb architecture, per-operation replies no longer
//! carry a universal `SignalVerb` tag. The per-operation reply is
//! positionally addressed -- its index in the `per_operation`
//! [`NonEmpty`] matches the index in the originating request's
//! operation sequence.
//!
//! Contract-domain rejection is a per-operation
//! `SubReply::Failed { detail: Some(<contract reply>) }` inside
//! `Reply::Accepted { outcome: AcceptedOutcome::Aborted, ... }`, not
//! a kernel-level `Reply::Rejected`. The kernel rejection surface is
//! reserved for true frame-level or receiver-internal failures.
//!
//! The `OperationFailureReason` taxonomy therefore names operation
//! abort causes that still have per-operation reply structure.
//! Infrastructure failures from a daemon's execution engine remain
//! kernel-shaped rejections; their typed cause stays daemon-side.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use thiserror::Error;

use crate::non_empty::NonEmpty;

/// Reply is a typed sum. Pre-execution rejection and in-execution
/// abort are different categories; splitting the variants makes
/// illegal states unrepresentable.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum Reply<ReplyPayload> {
    /// Request was accepted for execution. Per-operation results follow.
    /// `outcome` distinguishes all-committed from aborted-at-N.
    Accepted {
        outcome: AcceptedOutcome,
        per_operation: NonEmpty<SubReply<ReplyPayload>>,
    },
    /// Request was rejected before any operation began (true frame /
    /// kernel failure: decode error, malformed shape, daemon-internal
    /// pre-execution failure, version skew). No per-operation results
    /// because no operation ran. Domain-level rejections by the daemon
    /// never appear here -- they ride as
    /// `Reply::Accepted { outcome: Aborted, per_operation: [..., Failed
    /// { detail: Some(contract_reply) }, ...] }`.
    Rejected { reason: RequestRejectionReason },
}

impl<ReplyPayload> Reply<ReplyPayload> {
    /// Build a fully committed reply.
    pub fn committed(per_operation: NonEmpty<SubReply<ReplyPayload>>) -> Self {
        Self::Accepted {
            outcome: AcceptedOutcome::Committed,
            per_operation,
        }
    }

    /// Build an aborted reply (a contract-domain rejection or engine
    /// rejection that surfaces as per-operation `Failed`/`Invalidated`/
    /// `Skipped`).
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

    /// Build a true kernel rejection. Reserved for frame-level failures;
    /// see [`Reply::Rejected`] for the discipline.
    pub fn rejected(reason: RequestRejectionReason) -> Self {
        Self::Rejected { reason }
    }
}

/// Discriminates a fully committed reply from an aborted one.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum AcceptedOutcome {
    /// Every operation in the request committed.
    Committed,
    /// An operation at `failed_at` failed; the request aborted.
    Aborted {
        failed_at: usize,
        reason: OperationFailureReason,
    },
}

/// Why a request was rejected before any operation ran. Frame-layer
/// rkyv decode failures are protocol errors (close + log), not typed
/// rejections -- they have no `ExchangeIdentifier` to address.
///
/// Under the contract-local-verb architecture the former universal
/// rejection reasons (verb/payload mismatch, Subscribe-out-of-position)
/// no longer apply. Daemon-internal pre-execution policies surface as
/// `Internal`.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RequestRejectionReason {
    /// Receiver-internal error before any operation ran. Also covers
    /// engine-level infrastructure failure (atomic commit returned a
    /// typed error; the typed cause stays daemon-side, the wire reply
    /// is kernel-shaped).
    #[error("receiver-internal pre-execution or infrastructure failure")]
    Internal,
}

/// Why an operation failed during execution.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum OperationFailureReason {
    /// The contract's domain rejected the operation. The typed contract
    /// reply variant lives in
    /// `SubReply::Failed { detail: Some(reply) }`.
    #[error("domain rejected the operation")]
    DomainRejection,
}

/// Per-operation reply variant. Each variant carries only the fields a
/// reader needs, so illegal combinations (Ok with no payload, Skipped
/// with payload) are unrepresentable.
///
/// Per-op replies are positionally addressed -- the index in the
/// `per_operation` [`NonEmpty`] aligns with the originating request's
/// operation index. No universal verb tag is carried.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum SubReply<ReplyPayload> {
    /// Operation ran and committed/completed; only emitted under
    /// [`AcceptedOutcome::Committed`].
    Ok(ReplyPayload),
    /// Operation was admitted into the request -- either it ran and its
    /// result is no longer authoritative because the request as a whole
    /// aborted, OR it was planned/lowered but invalidated before commit
    /// because a sibling operation rejected the request. Both shapes
    /// witness "this operation contributed nothing durable" without
    /// distinguishing whether its work was executed and rolled back vs
    /// never reached the engine.
    Invalidated,
    /// Operation was attempted and failed; this is the cause of the abort.
    /// Exactly one per [`AcceptedOutcome::Aborted`] reply, at
    /// `failed_at`. `detail` carries the typed contract reply for
    /// [`OperationFailureReason::DomainRejection`].
    Failed {
        reason: OperationFailureReason,
        detail: Option<ReplyPayload>,
    },
    /// Operation never ran because an earlier operation failed.
    Skipped,
}
