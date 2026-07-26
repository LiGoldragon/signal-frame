use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum OperationDispatchError {
    #[error("unknown operation root byte {root}")]
    UnknownOperationRoot { root: u8 },
    #[error("unknown operation route variant {variant} for root {root}")]
    UnknownOperationVariant { root: u8, variant: u8 },
    #[error("short-header route mismatch: expected {expected:?}, decoded {actual:?}")]
    HeaderRouteMismatch {
        expected: crate::WireRoute,
        actual: crate::WireRoute,
    },
    #[error("short-header operation root mismatch: expected {expected}, decoded {actual}")]
    HeaderOperationMismatch { expected: u8, actual: u8 },
}
