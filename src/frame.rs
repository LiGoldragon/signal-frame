use std::marker::PhantomData;

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};

use crate::binding::{
    ContractBinding, ContractId, RootCode, VariantCode, WireContract, WireRevision, WireRoute,
};
use crate::error::FrameError;
use crate::exchange::{ExchangeIdentifier, StreamEventIdentifier};
use crate::reply::Reply;
use crate::request::Request;
use crate::subscription::SubscriptionTokenInner;
use crate::version::{HandshakeReply, HandshakeRequest};

pub const SHORT_HEADER_BYTE_COUNT: usize = 8;

/// The mandatory 64-bit pre-typed frame prefix.
///
/// The length-prefixed wire shape is:
///
/// `u32 length` -> little-endian `ShortHeader` bytes -> archived frame body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShortHeader(u64);

impl ShortHeader {
    const fn bound(binding: ContractBinding, route: WireRoute) -> Self {
        Self(
            binding.contract().value() as u64
                | ((binding.revision().value() as u64) << 32)
                | ((route.variant().value() as u64) << 48)
                | ((route.root().value() as u64) << 56),
        )
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    /// Read a non-legacy binding from this header.
    ///
    /// A zero prefix is represented only as [`FrameError::UnboundHeader`];
    /// it never materializes a `ContractId` or `WireRevision`.
    pub const fn binding(self) -> ContractBinding {
        let Ok(contract) = ContractId::try_new(self.0 as u32) else {
            unreachable!()
        };
        let Ok(revision) = WireRevision::try_new((self.0 >> 32) as u16) else {
            unreachable!()
        };
        ContractBinding::new(contract, revision)
    }

    pub const fn route(self) -> WireRoute {
        WireRoute::new(
            RootCode::new((self.0 >> 56) as u8),
            VariantCode::new((self.0 >> 48) as u8),
        )
    }

    pub const fn to_le_bytes(self) -> [u8; SHORT_HEADER_BYTE_COUNT] {
        self.0.to_le_bytes()
    }
}

/// Parsed external prefix bytes which have not yet been accepted as a
/// production contract binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RawShortHeader(u64);

impl RawShortHeader {
    fn from_le_bytes(bytes: [u8; SHORT_HEADER_BYTE_COUNT]) -> Self {
        Self(u64::from_le_bytes(bytes))
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub const fn validate(self) -> Result<ShortHeader, FrameError> {
        if ContractId::try_new(self.0 as u32).is_err()
            || WireRevision::try_new((self.0 >> 32) as u16).is_err()
        {
            return Err(FrameError::UnboundHeader);
        }
        Ok(ShortHeader(self.0))
    }
}

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
    /// outbound lane. Publisher state is responsible for allocating the
    /// successive [`crate::LaneSequence`] values.
    SubscriptionEvent {
        event_identifier: StreamEventIdentifier,
        token: SubscriptionTokenInner,
        event: EventPayload,
    },
}

/// Production exchange frame whose header binding is derived from `Contract`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundExchangeFrame<Contract, RequestPayload, ReplyPayload> {
    short_header: ShortHeader,
    body: ExchangeFrameBody<RequestPayload, ReplyPayload>,
    contract: PhantomData<fn() -> Contract>,
}

/// Production streaming frame whose header binding is derived from `Contract`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundStreamingFrame<Contract, RequestPayload, ReplyPayload, EventPayload> {
    short_header: ShortHeader,
    body: StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload>,
    contract: PhantomData<fn() -> Contract>,
}

impl<Contract, RequestPayload, ReplyPayload>
    BoundExchangeFrame<Contract, RequestPayload, ReplyPayload>
where
    Contract: WireContract,
{
    pub fn new(route: WireRoute, body: ExchangeFrameBody<RequestPayload, ReplyPayload>) -> Self {
        Self {
            short_header: ShortHeader::bound(Contract::BINDING, route),
            body,
            contract: PhantomData,
        }
    }

    pub fn short_header(&self) -> ShortHeader {
        self.short_header
    }

    pub fn body(&self) -> &ExchangeFrameBody<RequestPayload, ReplyPayload> {
        &self.body
    }

    pub fn into_body(self) -> ExchangeFrameBody<RequestPayload, ReplyPayload> {
        self.body
    }
}

impl<Contract, RequestPayload, ReplyPayload, EventPayload>
    BoundStreamingFrame<Contract, RequestPayload, ReplyPayload, EventPayload>
where
    Contract: WireContract,
{
    pub fn new(
        route: WireRoute,
        body: StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload>,
    ) -> Self {
        Self {
            short_header: ShortHeader::bound(Contract::BINDING, route),
            body,
            contract: PhantomData,
        }
    }

    pub fn short_header(&self) -> ShortHeader {
        self.short_header
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
        .map_err(FrameError::ArchiveSerialize)
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

pub fn short_header_from_archive(bytes: &[u8]) -> Result<RawShortHeader, FrameError> {
    let header = bytes
        .get(..SHORT_HEADER_BYTE_COUNT)
        .ok_or(FrameError::ShortHeaderTooShort { found: bytes.len() })?;
    let mut header_bytes = [0; SHORT_HEADER_BYTE_COUNT];
    header_bytes.copy_from_slice(header);
    Ok(RawShortHeader::from_le_bytes(header_bytes))
}

pub fn short_header_from_length_prefixed(bytes: &[u8]) -> Result<RawShortHeader, FrameError> {
    short_header_from_archive(strip_length_prefix(bytes)?)
}

fn encode_frame_archive<Body>(short_header: ShortHeader, body: &Body) -> Result<Vec<u8>, FrameError>
where
    Body: Archive + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
{
    let body_archive = encode_archive(body)?;
    let mut frame_archive = Vec::with_capacity(SHORT_HEADER_BYTE_COUNT + body_archive.len());
    frame_archive.extend_from_slice(&short_header.to_le_bytes());
    frame_archive.extend_from_slice(&body_archive);
    Ok(frame_archive)
}

fn strip_short_header(bytes: &[u8]) -> Result<(RawShortHeader, &[u8]), FrameError> {
    let short_header = short_header_from_archive(bytes)?;
    Ok((short_header, &bytes[SHORT_HEADER_BYTE_COUNT..]))
}

impl ContractBinding {
    /// Validate a raw frame prefix before allowing its archive body to be read.
    pub fn decode_archive_with<Value>(
        self,
        bytes: &[u8],
        decoder: impl FnOnce(&[u8]) -> Result<Value, FrameError>,
    ) -> Result<Value, FrameError> {
        let (raw_header, body_archive) = strip_short_header(bytes)?;
        self.validate_header(raw_header.validate()?)?;
        decoder(body_archive)
    }

    /// Validate a length and contract header before allowing archive decoding.
    pub fn decode_length_prefixed_with<Value>(
        self,
        bytes: &[u8],
        decoder: impl FnOnce(&[u8]) -> Result<Value, FrameError>,
    ) -> Result<Value, FrameError> {
        self.decode_archive_with(strip_length_prefix(bytes)?, decoder)
    }
}

impl<Contract, RequestPayload, ReplyPayload>
    BoundExchangeFrame<Contract, RequestPayload, ReplyPayload>
where
    Contract: WireContract,
    RequestPayload: Archive + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
    ReplyPayload: Archive + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
{
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        encode_frame_archive(self.short_header, &self.body)
    }

    pub fn encode_length_prefixed(&self) -> Result<Vec<u8>, FrameError> {
        length_prefix(self.encode()?)
    }
}

impl<Contract, RequestPayload, ReplyPayload, EventPayload>
    BoundStreamingFrame<Contract, RequestPayload, ReplyPayload, EventPayload>
where
    Contract: WireContract,
    RequestPayload: Archive + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
    ReplyPayload: Archive + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
    EventPayload: Archive + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
{
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        encode_frame_archive(self.short_header, &self.body)
    }

    pub fn encode_length_prefixed(&self) -> Result<Vec<u8>, FrameError> {
        length_prefix(self.encode()?)
    }
}

impl<Contract, RequestPayload, ReplyPayload>
    BoundExchangeFrame<Contract, RequestPayload, ReplyPayload>
where
    Contract: WireContract,
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
        Contract::BINDING.decode_archive_with(bytes, |body_archive| {
            let body = rkyv::from_bytes::<
                ExchangeFrameBody<RequestPayload, ReplyPayload>,
                rkyv::rancor::Error,
            >(body_archive)
            .map_err(FrameError::ArchiveDeserialize)?;
            let header = short_header_from_archive(bytes)?.validate()?;
            Ok(Self::new(header.route(), body))
        })
    }

    pub fn decode_length_prefixed(bytes: &[u8]) -> Result<Self, FrameError> {
        let archive = strip_length_prefix(bytes)?;
        Self::decode(archive)
    }
}

impl<Contract, RequestPayload, ReplyPayload, EventPayload>
    BoundStreamingFrame<Contract, RequestPayload, ReplyPayload, EventPayload>
where
    Contract: WireContract,
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
        Contract::BINDING.decode_archive_with(bytes, |body_archive| {
            let body = rkyv::from_bytes::<
                StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload>,
                rkyv::rancor::Error,
            >(body_archive)
            .map_err(FrameError::ArchiveDeserialize)?;
            let header = short_header_from_archive(bytes)?.validate()?;
            Ok(Self::new(header.route(), body))
        })
    }

    pub fn decode_length_prefixed(bytes: &[u8]) -> Result<Self, FrameError> {
        let archive = strip_length_prefix(bytes)?;
        Self::decode(archive)
    }
}
