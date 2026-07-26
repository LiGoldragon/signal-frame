use thiserror::Error;

use crate::{ContractId, WireRevision, WireRouteError};

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("rkyv archive serialization failed")]
    ArchiveSerialize(#[source] rkyv::rancor::Error),

    #[error("rkyv archive deserialization failed")]
    ArchiveDeserialize(#[source] rkyv::rancor::Error),

    #[error("frame is shorter than the four byte length prefix")]
    ShortLengthPrefix,

    #[error("frame payload is shorter than the eight byte short header: found {found} bytes")]
    ShortHeaderTooShort { found: usize },

    #[error("frame length mismatch: expected {expected} bytes, found {found}")]
    LengthMismatch { expected: usize, found: usize },

    #[error("raw frame header has no production contract binding")]
    UnboundHeader,

    #[error("frame contract mismatch: expected {expected:?}, found {found:?}")]
    ContractMismatch {
        expected: ContractId,
        found: ContractId,
    },

    #[error(
        "unsupported wire revision for contract {contract:?}: expected {expected:?}, found {found:?}"
    )]
    UnsupportedWireRevision {
        contract: ContractId,
        expected: WireRevision,
        found: WireRevision,
    },

    #[error(transparent)]
    InvalidRoute(#[from] WireRouteError),
}
