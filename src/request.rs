//! `Request<Payload>` — one or more contract payloads carried in a frame.
//!
//! Under the contract-local-verb architecture, the universal
//! `SignalVerb` tag has been removed; with it, the previous
//! `Operation<Payload>` wrapper has also been collapsed out. A request
//! is directly a `NonEmpty<Payload>` — each payload is itself a
//! contract operation, and the payload's NOTA record head names the
//! contract-local verb.
//!
//! `RequestPayload` is a marker trait. Channel-specific structural
//! rules (e.g. ordering, multi-operation atomicity) belong to the
//! daemon executor or to payload constructors — not to a kernel
//! validation function that lies about doing work.

use nota_codec::{
    Decoder, Encoder, Error as NotaError, NotaDecode, NotaEncode, Result as NotaResult, Token,
};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use thiserror::Error;

use crate::caller::Caller;
use crate::non_empty::{NonEmpty, NonEmptyError};

/// One or more contract payloads that commit (or abort) as one unit.
///
/// The single-payload case is a length-1 [`NonEmpty`]; multi-payload
/// requests carry additional elements. Atomicity is structural — the
/// `NonEmpty<Payload>` is the unit.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct Request<Payload> {
    pub caller: Option<Caller>,
    pub payloads: NonEmpty<Payload>,
}

/// Marker trait implemented by every per-channel request payload enum
/// the `signal_channel!` macro emits. Carries the
/// [`Request::from_payload`] convenience that wraps a payload into a
/// length-1 `Request`.
pub trait RequestPayload: Sized {
    /// Default single-payload convenience: payload becomes a length-1
    /// [`Request`] via this method.
    fn into_request(self) -> Request<Self> {
        Request::from_payload(self)
    }
}

impl<Payload> Request<Payload> {
    pub fn from_payloads(payloads: NonEmpty<Payload>) -> Self {
        Self {
            caller: None,
            payloads,
        }
    }

    pub fn payloads(&self) -> &NonEmpty<Payload> {
        &self.payloads
    }

    pub fn caller(&self) -> Option<&Caller> {
        self.caller.as_ref()
    }

    pub fn with_caller(mut self, caller: Option<Caller>) -> Self {
        self.caller = caller;
        self
    }

    pub fn from_payload(payload: Payload) -> Self {
        Self {
            caller: None,
            payloads: NonEmpty::single(payload),
        }
    }
}

/// Ergonomic multi-payload request builder.
pub struct RequestBuilder<Payload> {
    payloads: Vec<Payload>,
}

impl<Payload> Default for RequestBuilder<Payload> {
    fn default() -> Self {
        Self {
            payloads: Vec::new(),
        }
    }
}

impl<Payload> RequestBuilder<Payload> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, payload: Payload) -> Self {
        self.payloads.push(payload);
        self
    }

    pub fn push(&mut self, payload: Payload) {
        self.payloads.push(payload);
    }

    pub fn build(self) -> std::result::Result<Request<Payload>, RequestBuilderError> {
        match NonEmpty::try_from_vec(self.payloads) {
            Ok(payloads) => Ok(Request {
                caller: None,
                payloads,
            }),
            Err(NonEmptyError::Empty) => Err(RequestBuilderError::EmptyRequest),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RequestBuilderError {
    #[error("cannot build a Request from an empty payload list")]
    EmptyRequest,
}

/// NOTA codec: a length-1 `Request` encodes as its single payload
/// directly (the payload's own record head names the contract-local
/// verb); a length-N request encodes as a bracketed sequence
/// `[(Verb ...) (Verb ...) ...]`. Decode peeks the next token — `[`
/// means sequence, anything else means single payload.
impl<Payload> NotaEncode for Request<Payload>
where
    Payload: NotaEncode,
{
    fn encode(&self, encoder: &mut Encoder) -> NotaResult<()> {
        if self.payloads.tail().is_empty() {
            self.payloads.head().encode(encoder)
        } else {
            encoder.start_seq()?;
            for payload in self.payloads.iter() {
                payload.encode(encoder)?;
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
                let mut payloads = Vec::new();
                while !decoder.peek_is_seq_end()? {
                    payloads.push(Payload::decode(decoder)?);
                }
                decoder.expect_seq_end()?;
                match NonEmpty::try_from_vec(payloads) {
                    Ok(payloads) => Ok(Request {
                        caller: None,
                        payloads,
                    }),
                    Err(NonEmptyError::Empty) => Err(NotaError::Validation {
                        type_name: "Request",
                        message: "empty payload sequence".into(),
                    }),
                }
            }
            Some(_) => {
                let payload = Payload::decode(decoder)?;
                Ok(Request {
                    caller: None,
                    payloads: NonEmpty::single(payload),
                })
            }
            None => Err(NotaError::UnexpectedEnd {
                while_parsing: "Request",
            }),
        }
    }
}
