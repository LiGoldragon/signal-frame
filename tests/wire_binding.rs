use std::{
    cell::Cell,
    mem::size_of,
    num::{NonZeroU16, NonZeroU32},
};

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_frame::{
    BoundExchangeFrame, BoundStreamingFrame, ContractBinding, ContractId, ExchangeFrameBody,
    ExchangeIdentifier, ExchangeLane, FrameError, HandshakeRequest, LaneSequence, RootCode,
    SessionEpoch, ShortHeader, StreamEventIdentifier, StreamingFrameBody, SubscriptionTokenInner,
    VariantCode, WireContract, WireRevision, WireRoute, WireRouteError, short_header_from_archive,
};

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct RequestPayload(u32);
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct ReplyPayload(u32);
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct EventPayload(u32);

struct OrdinaryContract;
impl WireContract for OrdinaryContract {
    const BINDING: ContractBinding = ContractBinding::new(
        ContractId::new(NonZeroU32::new(0x1020_3040).unwrap()),
        WireRevision::new(NonZeroU16::new(7).unwrap()),
    );
}
struct OtherContract;
impl WireContract for OtherContract {
    const BINDING: ContractBinding = ContractBinding::new(
        ContractId::new(NonZeroU32::new(0x5060_7080).unwrap()),
        WireRevision::new(NonZeroU16::new(7).unwrap()),
    );
}
struct NewerContract;
impl WireContract for NewerContract {
    const BINDING: ContractBinding = ContractBinding::new(
        ContractId::new(NonZeroU32::new(0x1020_3040).unwrap()),
        WireRevision::new(NonZeroU16::new(8).unwrap()),
    );
}
struct MaximumContract;
impl WireContract for MaximumContract {
    const BINDING: ContractBinding = ContractBinding::new(
        ContractId::new(NonZeroU32::MAX),
        WireRevision::new(NonZeroU16::MAX),
    );
}

const ROUTE: WireRoute = WireRoute::new(RootCode::new(0x91), VariantCode::new(0x2a));

fn header<Contract: WireContract>(route: WireRoute) -> ShortHeader {
    BoundExchangeFrame::<Contract, RequestPayload, ReplyPayload>::new(
        route,
        ExchangeFrameBody::HandshakeRequest(HandshakeRequest::current()),
    )
    .short_header()
}

fn exchange(sequence: u64) -> ExchangeIdentifier {
    ExchangeIdentifier::new(
        SessionEpoch::new(11),
        ExchangeLane::Connector,
        LaneSequence::new(sequence),
    )
}

#[test]
fn bound_header_packing_is_exact_and_contract_collisions_do_not_alias() {
    let ordinary = header::<OrdinaryContract>(ROUTE);
    let other = header::<OtherContract>(ROUTE);
    assert_eq!(
        ordinary.to_le_bytes(),
        [0x40, 0x30, 0x20, 0x10, 0x07, 0x00, 0x2a, 0x91]
    );
    assert_eq!(ordinary.binding(), OrdinaryContract::BINDING);
    assert_eq!(ordinary.route(), ROUTE);
    assert_ne!(ordinary, other);
    assert_ne!(ordinary.binding(), other.binding());

    let maximum = header::<MaximumContract>(WireRoute::new(
        RootCode::new(u8::MAX),
        VariantCode::new(u8::MAX),
    ));
    assert_eq!(maximum.value(), u64::MAX);
}

#[test]
fn raw_zero_header_never_materializes_as_a_short_header() {
    let raw = short_header_from_archive(&[0; 8]).unwrap();
    assert_eq!(raw.value(), 0);
    assert!(matches!(raw.validate(), Err(FrameError::UnboundHeader)));
    assert!(ContractId::try_new(0).is_err());
    assert!(WireRevision::try_new(0).is_err());
}

#[test]
fn wrong_binding_rejects_before_archive_decoder_sentinel() {
    for (bytes, expected) in [
        (
            header::<OtherContract>(ROUTE).to_le_bytes(),
            FrameError::ContractMismatch {
                expected: OrdinaryContract::BINDING.contract(),
                found: OtherContract::BINDING.contract(),
            },
        ),
        (
            header::<NewerContract>(ROUTE).to_le_bytes(),
            FrameError::UnsupportedWireRevision {
                contract: OrdinaryContract::BINDING.contract(),
                expected: OrdinaryContract::BINDING.revision(),
                found: NewerContract::BINDING.revision(),
            },
        ),
    ] {
        let mut archive = bytes.to_vec();
        archive.extend_from_slice(b"not an archive");
        let invoked = Cell::new(false);
        let error = OrdinaryContract::BINDING
            .decode_archive_with(&archive, |_| {
                invoked.set(true);
                Ok(())
            })
            .unwrap_err();
        assert!(!invoked.get());
        assert_eq!(error.to_string(), expected.to_string());
    }
}

#[test]
fn request_reply_control_and_push_round_trip_with_bound_identity() {
    let exchange = exchange(17);
    let request = BoundExchangeFrame::<OrdinaryContract, RequestPayload, ReplyPayload>::new(
        ROUTE,
        ExchangeFrameBody::Request {
            exchange,
            request: signal_frame::Request::from_payload(RequestPayload(41)),
        },
    );
    let decoded = BoundExchangeFrame::<OrdinaryContract, RequestPayload, ReplyPayload>::
        decode_length_prefixed(&request.encode_length_prefixed().unwrap()).unwrap();
    assert!(matches!(
        decoded.into_body(),
        ExchangeFrameBody::Request { exchange: found, .. } if found == exchange
    ));

    let event_identifier =
        StreamEventIdentifier::acceptor(SessionEpoch::new(23), LaneSequence::new(29));
    let stream =
        BoundStreamingFrame::<OrdinaryContract, RequestPayload, ReplyPayload, EventPayload>::new(
            ROUTE,
            StreamingFrameBody::SubscriptionEvent {
                event_identifier,
                token: SubscriptionTokenInner::new(31),
                event: EventPayload(37),
            },
        );
    let decoded =
        BoundStreamingFrame::<OrdinaryContract, RequestPayload, ReplyPayload, EventPayload>::
            decode_length_prefixed(&stream.encode_length_prefixed().unwrap()).unwrap();
    assert!(matches!(
        decoded.into_body(),
        StreamingFrameBody::SubscriptionEvent { event_identifier: found, .. }
            if found == event_identifier
                && found.lane() == ExchangeLane::Acceptor
                && found.sequence() == LaneSequence::new(29)
    ));
}

#[test]
fn route_projection_rejects_every_bit_outside_root_and_variant() {
    assert_eq!(
        WireRoute::try_from_log_variant(0xffff).unwrap(),
        WireRoute::new(RootCode::new(0xff), VariantCode::new(0xff))
    );
    for value in [1_u64 << 16, 1_u64 << 31, 1_u64 << 63, u64::MAX] {
        assert_eq!(
            WireRoute::try_from_log_variant(value),
            Err(WireRouteError::BitsOutsideRoute { value })
        );
    }
}

#[test]
fn primitive_layouts_remain_stable_widths_without_archivable_header_values() {
    assert_eq!(size_of::<ContractId>(), 4);
    assert_eq!(size_of::<WireRevision>(), 2);
    assert_eq!(size_of::<RootCode>(), 1);
    assert_eq!(size_of::<VariantCode>(), 1);
    assert_eq!(size_of::<ShortHeader>(), 8);
}
