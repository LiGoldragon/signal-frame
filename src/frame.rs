use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};

use crate::error::FrameError;
use crate::exchange::{ExchangeIdentifier, StreamEventIdentifier};
use crate::reply::Reply;
use crate::request::Request;
use crate::subscription::SubscriptionTokenInner;
use crate::version::{HandshakeReply, HandshakeRequest};

/// Frame body for an exchange-only channel — handshake + request/reply
/// only. No `SubscriptionEvent` variant; channels that stream events
/// use [`StreamingFrameBody`] instead.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum ExchangeFrameBody<RequestPayload, ReplyPayload> {
    HandshakeRequest(HandshakeRequest),
    HandshakeReply(HandshakeReply),
    Request {
        exchange: ExchangeIdentifier,
        request: Request<RequestPayload>,
    },
    Reply {
        exchange: ExchangeIdentifier,
        reply: Reply<ReplyPayload>,
    },
}

/// Frame body for a streaming channel — adds daemon-initiated
/// subscription events. The `event` payload is type-distinct from
/// reply payloads, so an event variant accidentally appearing inside a
/// reply (or vice versa) is unrepresentable.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload> {
    HandshakeRequest(HandshakeRequest),
    HandshakeReply(HandshakeReply),
    Request {
        exchange: ExchangeIdentifier,
        request: Request<RequestPayload>,
    },
    Reply {
        exchange: ExchangeIdentifier,
        reply: Reply<ReplyPayload>,
    },
    /// Daemon-initiated subscription event. Rides on the acceptor's
    /// outbound lane with its own monotonic [`crate::LaneSequence`].
    SubscriptionEvent {
        event_identifier: StreamEventIdentifier,
        token: SubscriptionTokenInner,
        event: EventPayload,
    },
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct ExchangeFrame<RequestPayload, ReplyPayload> {
    pub body: ExchangeFrameBody<RequestPayload, ReplyPayload>,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct StreamingFrame<RequestPayload, ReplyPayload, EventPayload> {
    pub body: StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload>,
}

impl<RequestPayload, ReplyPayload> ExchangeFrame<RequestPayload, ReplyPayload> {
    pub fn new(body: ExchangeFrameBody<RequestPayload, ReplyPayload>) -> Self {
        Self { body }
    }

    pub fn body(&self) -> &ExchangeFrameBody<RequestPayload, ReplyPayload> {
        &self.body
    }

    pub fn into_body(self) -> ExchangeFrameBody<RequestPayload, ReplyPayload> {
        self.body
    }
}

impl<RequestPayload, ReplyPayload, EventPayload>
    StreamingFrame<RequestPayload, ReplyPayload, EventPayload>
{
    pub fn new(body: StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload>) -> Self {
        Self { body }
    }

    pub fn body(&self) -> &StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload> {
        &self.body
    }

    pub fn into_body(self) -> StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload> {
        self.body
    }
}

type HighSerializer<'archive> = rkyv::api::high::HighSerializer<
    rkyv::util::AlignedVec,
    rkyv::ser::allocator::ArenaHandle<'archive>,
    rkyv::rancor::Error,
>;

fn encode_archive<Value>(value: &Value) -> Result<Vec<u8>, FrameError>
where
    Value: Archive + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
{
    rkyv::to_bytes::<rkyv::rancor::Error>(value)
        .map(|bytes| bytes.to_vec())
        .map_err(|_| FrameError::ArchiveValidation)
}

fn length_prefix(archive: Vec<u8>) -> Result<Vec<u8>, FrameError> {
    let length = u32::try_from(archive.len()).map_err(|_| FrameError::LengthMismatch {
        expected: u32::MAX as usize,
        found: archive.len(),
    })?;
    let mut framed = Vec::with_capacity(4 + archive.len());
    framed.extend_from_slice(&length.to_be_bytes());
    framed.extend_from_slice(&archive);
    Ok(framed)
}

fn strip_length_prefix(bytes: &[u8]) -> Result<&[u8], FrameError> {
    if bytes.len() < 4 {
        return Err(FrameError::ShortLengthPrefix);
    }
    let expected = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let payload = &bytes[4..];
    if payload.len() != expected {
        return Err(FrameError::LengthMismatch {
            expected,
            found: payload.len(),
        });
    }
    Ok(payload)
}

impl<RequestPayload, ReplyPayload> ExchangeFrame<RequestPayload, ReplyPayload>
where
    RequestPayload: Archive + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
    ReplyPayload: Archive + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
{
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        encode_archive(self)
    }

    pub fn encode_length_prefixed(&self) -> Result<Vec<u8>, FrameError> {
        length_prefix(self.encode()?)
    }
}

impl<RequestPayload, ReplyPayload, EventPayload>
    StreamingFrame<RequestPayload, ReplyPayload, EventPayload>
where
    RequestPayload: Archive + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
    ReplyPayload: Archive + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
    EventPayload: Archive + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
{
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        encode_archive(self)
    }

    pub fn encode_length_prefixed(&self) -> Result<Vec<u8>, FrameError> {
        length_prefix(self.encode()?)
    }
}

impl<RequestPayload, ReplyPayload> ExchangeFrame<RequestPayload, ReplyPayload>
where
    RequestPayload: Archive,
    ReplyPayload: Archive,
    <RequestPayload as Archive>::Archived: for<'archive> rkyv::bytecheck::CheckBytes<
            rkyv::api::high::HighValidator<'archive, rkyv::rancor::Error>,
        > + RkyvDeserialize<RequestPayload, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
    <ReplyPayload as Archive>::Archived: for<'archive> rkyv::bytecheck::CheckBytes<
            rkyv::api::high::HighValidator<'archive, rkyv::rancor::Error>,
        > + RkyvDeserialize<ReplyPayload, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
{
    pub fn decode(bytes: &[u8]) -> Result<Self, FrameError> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes)
            .map_err(|_| FrameError::ArchiveDeserialize)
    }

    pub fn decode_length_prefixed(bytes: &[u8]) -> Result<Self, FrameError> {
        Self::decode(strip_length_prefix(bytes)?)
    }
}

impl<RequestPayload, ReplyPayload, EventPayload>
    StreamingFrame<RequestPayload, ReplyPayload, EventPayload>
where
    RequestPayload: Archive,
    ReplyPayload: Archive,
    EventPayload: Archive,
    <RequestPayload as Archive>::Archived: for<'archive> rkyv::bytecheck::CheckBytes<
            rkyv::api::high::HighValidator<'archive, rkyv::rancor::Error>,
        > + RkyvDeserialize<RequestPayload, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
    <ReplyPayload as Archive>::Archived: for<'archive> rkyv::bytecheck::CheckBytes<
            rkyv::api::high::HighValidator<'archive, rkyv::rancor::Error>,
        > + RkyvDeserialize<ReplyPayload, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
    <EventPayload as Archive>::Archived: for<'archive> rkyv::bytecheck::CheckBytes<
            rkyv::api::high::HighValidator<'archive, rkyv::rancor::Error>,
        > + RkyvDeserialize<EventPayload, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
{
    pub fn decode(bytes: &[u8]) -> Result<Self, FrameError> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes)
            .map_err(|_| FrameError::ArchiveDeserialize)
    }

    pub fn decode_length_prefixed(bytes: &[u8]) -> Result<Self, FrameError> {
        Self::decode(strip_length_prefix(bytes)?)
    }
}
