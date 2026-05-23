use thiserror::Error;

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("rkyv archive validation failed")]
    ArchiveValidation,

    #[error("rkyv archive deserialization failed")]
    ArchiveDeserialize,

    #[error("frame is shorter than the four byte length prefix")]
    ShortLengthPrefix,

    #[error("frame payload is shorter than the eight byte short header: found {found} bytes")]
    ShortHeaderTooShort { found: usize },

    #[error("frame length mismatch: expected {expected} bytes, found {found}")]
    LengthMismatch { expected: usize, found: usize },
}
