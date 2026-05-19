//! `Request<Payload>` — one or more contract operations carried in a frame.
//!
//! Under the contract-local-verb architecture, the universal
//! `SignalVerb` tag has been removed. Each [`crate::Operation`]
//! carries only its payload; the payload's record head names the
//! contract-local verb.
//!
//! `RequestPayload` is now a marker trait. Channel-specific universal
//! rules (e.g. "Subscribe must be at the tail") are no longer encoded
//! here — the daemon executor enforces any structural rules at its own
//! layer, since the universal verb spine is gone.

use nota_codec::{
    Decoder, Encoder, Error as NotaError, NotaDecode, NotaEncode, Result as NotaResult, Token,
};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use thiserror::Error;

use crate::non_empty::{NonEmpty, NonEmptyError};
use crate::operation::Operation;
use crate::reply::RequestRejectionReason;

/// One or more contract operations that commit (or abort) as one unit.
///
/// The single-operation case is a length-1 [`NonEmpty`]; multi-operation
/// requests carry additional elements. Atomicity is structural — the
/// `NonEmpty<Operation<Payload>>` is the unit.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct Request<Payload> {
    pub operations: NonEmpty<Operation<Payload>>,
}

/// Marker trait implemented by every per-channel request payload enum
/// the `signal_channel!` macro emits. Carries the
/// [`Request::from_payload`] convenience that wraps a payload into a
/// length-1 `Request`.
pub trait RequestPayload: Sized {
    /// Default single-operation convenience: payload becomes a length-1
    /// [`Request`] via this method.
    fn into_request(self) -> Request<Self> {
        Request::from_payload(self)
    }
}

impl<Payload> Request<Payload> {
    pub fn from_operations(operations: NonEmpty<Operation<Payload>>) -> Self {
        Self { operations }
    }

    pub fn operations(&self) -> &NonEmpty<Operation<Payload>> {
        &self.operations
    }

    pub fn from_payload(payload: Payload) -> Self {
        Self {
            operations: NonEmpty::single(Operation { payload }),
        }
    }

    /// Universal structural rule check. Under the contract-local-verb
    /// architecture the universal rule set is empty: there is no
    /// universal verb tag to align against the payload, and no
    /// universal Subscribe-position rule (Subscribe is contract-local
    /// now). Channel-specific rules live in the daemon executor.
    ///
    /// Retained as a method so existing call sites that defensively
    /// validate before sending continue to compile; it returns `Ok(())`
    /// for every well-formed `Request`.
    pub fn check(&self) -> std::result::Result<(), RequestRejectionReason> {
        Ok(())
    }

    /// Consume on success; return `(reason, self)` on failure so the
    /// caller can build a rejection reply.
    pub fn into_checked(
        self,
    ) -> std::result::Result<CheckedRequest<Payload>, (RequestRejectionReason, Self)> {
        if let Err(reason) = self.check() {
            return Err((reason, self));
        }
        Ok(CheckedRequest {
            operations: self.operations,
        })
    }
}

/// A [`Request`] that has passed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedRequest<Payload> {
    pub operations: NonEmpty<Operation<Payload>>,
}

/// Ergonomic multi-operation request builder.
pub struct RequestBuilder<Payload> {
    operations: Vec<Operation<Payload>>,
}

impl<Payload> Default for RequestBuilder<Payload> {
    fn default() -> Self {
        Self {
            operations: Vec::new(),
        }
    }
}

impl<Payload> RequestBuilder<Payload> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, payload: Payload) -> Self {
        self.operations.push(Operation { payload });
        self
    }

    pub fn push(&mut self, payload: Payload) {
        self.operations.push(Operation { payload });
    }

    pub fn build(self) -> std::result::Result<Request<Payload>, RequestBuilderError> {
        match NonEmpty::try_from_vec(self.operations) {
            Ok(operations) => Ok(Request { operations }),
            Err(NonEmptyError::Empty) => Err(RequestBuilderError::EmptyRequest),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RequestBuilderError {
    #[error("cannot build a Request from an empty operation list")]
    EmptyRequest,
}

/// NOTA codec: a length-1 `Request` encodes as the single
/// [`Operation`] form (which delegates to the payload directly under
/// the contract-local-verb shape); a length-N request encodes as a
/// bracketed sequence `[(Op)(Op) ...]`. Decode peeks the next token —
/// `[` means sequence, anything else means single operation.
impl<Payload> NotaEncode for Request<Payload>
where
    Payload: NotaEncode,
{
    fn encode(&self, encoder: &mut Encoder) -> NotaResult<()> {
        if self.operations.tail().is_empty() {
            self.operations.head().encode(encoder)
        } else {
            encoder.start_seq()?;
            for operation in self.operations.iter() {
                operation.encode(encoder)?;
            }
            encoder.end_seq()
        }
    }
}

impl<Payload> NotaDecode for Request<Payload>
where
    Payload: NotaDecode,
{
    fn decode(decoder: &mut Decoder<'_>) -> NotaResult<Self> {
        match decoder.peek_token()? {
            Some(Token::LBracket) => {
                decoder.expect_seq_start()?;
                let mut operations = Vec::new();
                while !decoder.peek_is_seq_end()? {
                    operations.push(Operation::<Payload>::decode(decoder)?);
                }
                decoder.expect_seq_end()?;
                match NonEmpty::try_from_vec(operations) {
                    Ok(operations) => Ok(Request { operations }),
                    Err(NonEmptyError::Empty) => Err(NotaError::Validation {
                        type_name: "Request",
                        message: "empty operation sequence".into(),
                    }),
                }
            }
            Some(_) => {
                let operation = Operation::<Payload>::decode(decoder)?;
                Ok(Request {
                    operations: NonEmpty::single(operation),
                })
            }
            None => Err(NotaError::UnexpectedEnd {
                while_parsing: "Request",
            }),
        }
    }
}
