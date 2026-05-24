use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum OperationDispatchError {
    #[error("unknown operation root byte {root}")]
    UnknownOperationRoot { root: u8 },
    #[error("short-header operation root mismatch: expected {expected}, decoded {actual}")]
    HeaderOperationMismatch { expected: u8, actual: u8 },
}
