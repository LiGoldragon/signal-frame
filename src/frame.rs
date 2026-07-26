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
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, Debug, Default, Clone, Copy, PartialEq, Eq, Hash,
)]
pub struct ShortHeader(u64);

impl ShortHeader {
    /// Construct a raw compatibility header.
    ///
    /// Production-bound frames use [`ShortHeader::bound`] through
    /// [`BoundExchangeFrame`] or [`BoundStreamingFrame`].
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn bound(binding: ContractBinding, route: WireRoute) -> Self {
        Self(
            binding.contract().value() as u64
                | ((binding.revision().value() as u64) << 32)
                | ((route.variant().value() as u64) << 48)
                | ((route.root().value() as u64) << 56),
        )
    }

    /// Construct the only supported unbound shape for explicit legacy parsing.
    pub const fn legacy_unbound(route: WireRoute) -> Self {
        Self::bound(ContractBinding::legacy_unbound(), route)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub const fn binding(self) -> ContractBinding {
        ContractBinding::new(
            ContractId::from_header_bits(self.0 as u32),
            WireRevision::from_header_bits((self.0 >> 32) as u16),
        )
    }

    pub const fn route(self) -> WireRoute {
        WireRoute::new(
            RootCode::new((self.0 >> 56) as u8),
            VariantCode::new((self.0 >> 48) as u8),
        )
    }

    pub const fn is_legacy_unbound(self) -> bool {
        self.binding().is_legacy_unbound()
    }

    pub const fn to_le_bytes(self) -> [u8; SHORT_HEADER_BYTE_COUNT] {
        self.0.to_le_bytes()
    }

    pub const fn from_le_bytes(bytes: [u8; SHORT_HEADER_BYTE_COUNT]) -> Self {
        Self(u64::from_le_bytes(bytes))
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
    /// outbound lane with its own monotonic [`crate::LaneSequence`].
    SubscriptionEvent {
        event_identifier: StreamEventIdentifier,
        token: SubscriptionTokenInner,
        event: EventPayload,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeFrame<RequestPayload, ReplyPayload> {
    pub short_header: ShortHeader,
    pub body: ExchangeFrameBody<RequestPayload, ReplyPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingFrame<RequestPayload, ReplyPayload, EventPayload> {
    pub short_header: ShortHeader,
    pub body: StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload>,
}

/// Production exchange frame whose header binding is derived from `Contract`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundExchangeFrame<Contract, RequestPayload, ReplyPayload> {
    inner: ExchangeFrame<RequestPayload, ReplyPayload>,
    contract: PhantomData<fn() -> Contract>,
}

/// Production streaming frame whose header binding is derived from `Contract`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundStreamingFrame<Contract, RequestPayload, ReplyPayload, EventPayload> {
    inner: StreamingFrame<RequestPayload, ReplyPayload, EventPayload>,
    contract: PhantomData<fn() -> Contract>,
}

impl<RequestPayload, ReplyPayload> ExchangeFrame<RequestPayload, ReplyPayload> {
    pub fn new(body: ExchangeFrameBody<RequestPayload, ReplyPayload>) -> Self {
        Self::with_short_header(ShortHeader::empty(), body)
    }

    pub fn with_short_header(
        short_header: ShortHeader,
        body: ExchangeFrameBody<RequestPayload, ReplyPayload>,
    ) -> Self {
        Self { short_header, body }
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

impl<RequestPayload, ReplyPayload, EventPayload>
    StreamingFrame<RequestPayload, ReplyPayload, EventPayload>
{
    pub fn new(body: StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload>) -> Self {
        Self::with_short_header(ShortHeader::empty(), body)
    }

    pub fn with_short_header(
        short_header: ShortHeader,
        body: StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload>,
    ) -> Self {
        Self { short_header, body }
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

impl<Contract, RequestPayload, ReplyPayload>
    BoundExchangeFrame<Contract, RequestPayload, ReplyPayload>
where
    Contract: WireContract,
{
    pub fn new(route: WireRoute, body: ExchangeFrameBody<RequestPayload, ReplyPayload>) -> Self {
        Self {
            inner: ExchangeFrame::with_short_header(
                ShortHeader::bound(Contract::BINDING, route),
                body,
            ),
            contract: PhantomData,
        }
    }

    pub fn short_header(&self) -> ShortHeader {
        self.inner.short_header()
    }

    pub fn body(&self) -> &ExchangeFrameBody<RequestPayload, ReplyPayload> {
        self.inner.body()
    }

    pub fn into_body(self) -> ExchangeFrameBody<RequestPayload, ReplyPayload> {
        self.inner.into_body()
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
            inner: StreamingFrame::with_short_header(
                ShortHeader::bound(Contract::BINDING, route),
                body,
            ),
            contract: PhantomData,
        }
    }

    pub fn short_header(&self) -> ShortHeader {
        self.inner.short_header()
    }

    pub fn body(&self) -> &StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload> {
        self.inner.body()
    }

    pub fn into_body(self) -> StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload> {
        self.inner.into_body()
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

pub fn short_header_from_archive(bytes: &[u8]) -> Result<ShortHeader, FrameError> {
    let header = bytes
        .get(..SHORT_HEADER_BYTE_COUNT)
        .ok_or(FrameError::ShortHeaderTooShort { found: bytes.len() })?;
    let mut header_bytes = [0; SHORT_HEADER_BYTE_COUNT];
    header_bytes.copy_from_slice(header);
    Ok(ShortHeader::from_le_bytes(header_bytes))
}

pub fn short_header_from_length_prefixed(bytes: &[u8]) -> Result<ShortHeader, FrameError> {
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

fn strip_short_header(bytes: &[u8]) -> Result<(ShortHeader, &[u8]), FrameError> {
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
        let (header, body_archive) = strip_short_header(bytes)?;
        self.validate_header(header)?;
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

impl<RequestPayload, ReplyPayload> ExchangeFrame<RequestPayload, ReplyPayload>
where
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

impl<RequestPayload, ReplyPayload, EventPayload>
    StreamingFrame<RequestPayload, ReplyPayload, EventPayload>
where
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
    RequestPayload: Archive + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
    ReplyPayload: Archive + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
{
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        self.inner.encode()
    }

    pub fn encode_length_prefixed(&self) -> Result<Vec<u8>, FrameError> {
        self.inner.encode_length_prefixed()
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
        self.inner.encode()
    }

    pub fn encode_length_prefixed(&self) -> Result<Vec<u8>, FrameError> {
        self.inner.encode_length_prefixed()
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
        let (short_header, body_archive) = strip_short_header(bytes)?;
        let body = rkyv::from_bytes::<
            ExchangeFrameBody<RequestPayload, ReplyPayload>,
            rkyv::rancor::Error,
        >(body_archive)
        .map_err(|_| FrameError::ArchiveDeserialize)?;
        Ok(Self::with_short_header(short_header, body))
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
        let (short_header, body_archive) = strip_short_header(bytes)?;
        let body = rkyv::from_bytes::<
            StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload>,
            rkyv::rancor::Error,
        >(body_archive)
        .map_err(|_| FrameError::ArchiveDeserialize)?;
        Ok(Self::with_short_header(short_header, body))
    }

    pub fn decode_length_prefixed(bytes: &[u8]) -> Result<Self, FrameError> {
        Self::decode(strip_length_prefix(bytes)?)
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
            .map_err(|_| FrameError::ArchiveDeserialize)?;
            Ok(Self::new(short_header_from_archive(bytes)?.route(), body))
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
            .map_err(|_| FrameError::ArchiveDeserialize)?;
            Ok(Self::new(short_header_from_archive(bytes)?.route(), body))
        })
    }

    pub fn decode_length_prefixed(bytes: &[u8]) -> Result<Self, FrameError> {
        let archive = strip_length_prefix(bytes)?;
        Self::decode(archive)
    }
}
