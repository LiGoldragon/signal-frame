use thiserror::Error;

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("rkyv archive validation failed")]
    ArchiveValidation,

    #[error("rkyv archive deserialization failed")]
    ArchiveDeserialize,

    #[error("frame is shorter than the four byte length prefix")]
    ShortLengthPrefix,

    #[error("frame length mismatch: expected {expected} bytes, found {found}")]
    LengthMismatch { expected: usize, found: usize },
}
