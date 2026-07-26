use std::cell::Cell;
use std::mem::size_of;

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_frame::{
    BoundExchangeFrame, BoundStreamingFrame, ContractBinding, ContractId, ExchangeFrame,
    ExchangeFrameBody, ExchangeIdentifier, ExchangeLane, FrameError, HandshakeRequest,
    LaneSequence, RootCode, SessionEpoch, ShortHeader, StreamEventIdentifier, StreamingFrameBody,
    SubscriptionTokenInner, VariantCode, WireContract, WireRevision, WireRoute,
};

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct RequestPayload(u32);

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct ReplyPayload(u32);

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct EventPayload(u32);

#[derive(Debug)]
struct OrdinaryContract;

impl WireContract for OrdinaryContract {
    const BINDING: ContractBinding =
        ContractBinding::new(ContractId::new(0x1020_3040), WireRevision::new(7));
}

struct OtherContract;

impl WireContract for OtherContract {
    const BINDING: ContractBinding =
        ContractBinding::new(ContractId::new(0x5060_7080), WireRevision::new(7));
}

struct NewerOrdinaryContract;

impl WireContract for NewerOrdinaryContract {
    const BINDING: ContractBinding =
        ContractBinding::new(ContractId::new(0x1020_3040), WireRevision::new(8));
}

const ROUTE: WireRoute = WireRoute::new(RootCode::new(0x91), VariantCode::new(0x2a));

fn exchange(sequence: u64) -> ExchangeIdentifier {
    ExchangeIdentifier::new(
        SessionEpoch::new(11),
        ExchangeLane::Connector,
        LaneSequence::new(sequence),
    )
}

#[test]
fn contract_local_route_collision_is_separated_by_contract_identity() {
    let ordinary = ShortHeader::bound(OrdinaryContract::BINDING, ROUTE);
    let other = ShortHeader::bound(OtherContract::BINDING, ROUTE);

    assert_eq!(
        ordinary.to_le_bytes(),
        [0x40, 0x30, 0x20, 0x10, 0x07, 0x00, 0x2a, 0x91]
    );
    assert_ne!(ordinary, other);
    assert_eq!(ordinary.route(), other.route());
    assert_ne!(ordinary.binding(), other.binding());
}

#[test]
fn short_header_pack_unpack_is_injective_at_boundaries_and_property_cases() {
    let contracts = [1, u32::MAX, 0x5555_aaaa, 0xaaaa_5555];
    let revisions = [1, u16::MAX, 0x5555, 0xaaaa];
    let roots = [0, u8::MAX, 0x55, 0xaa];
    let variants = [0, u8::MAX, 0x55, 0xaa];
    let mut packed = std::collections::BTreeSet::new();

    for contract in contracts {
        for revision in revisions {
            for root in roots {
                for variant in variants {
                    let binding = ContractBinding::new(
                        ContractId::new(contract),
                        WireRevision::new(revision),
                    );
                    let route = WireRoute::new(RootCode::new(root), VariantCode::new(variant));
                    let header = ShortHeader::bound(binding, route);
                    assert_eq!(header.binding(), binding);
                    assert_eq!(header.route(), route);
                    assert!(packed.insert(header.value()));
                }
            }
        }
    }

    let boundary = ShortHeader::bound(
        ContractBinding::new(ContractId::new(u32::MAX), WireRevision::new(u16::MAX)),
        WireRoute::new(RootCode::new(u8::MAX), VariantCode::new(u8::MAX)),
    );
    assert_eq!(boundary.value(), u64::MAX);
}

#[test]
fn zero_is_reserved_and_legacy_unbound_is_explicit() {
    assert!(ContractId::try_new(0).is_err());
    assert!(WireRevision::try_new(0).is_err());

    let legacy = ShortHeader::legacy_unbound(ROUTE);
    assert!(legacy.is_legacy_unbound());
    assert!(legacy.binding().contract().is_legacy_unbound());
    assert!(legacy.binding().revision().is_legacy_unbound());
    assert_eq!(
        OrdinaryContract::BINDING.validate_header(legacy),
        Err(FrameError::LegacyUnboundHeader)
    );
}

#[test]
fn wrong_contract_rejects_before_archive_decoder_is_invoked() {
    let header = ShortHeader::bound(OtherContract::BINDING, ROUTE);
    let mut bytes = header.to_le_bytes().to_vec();
    bytes.extend_from_slice(b"not an archive");
    let invoked = Cell::new(false);

    let error = OrdinaryContract::BINDING
        .decode_archive_with(&bytes, |_| {
            invoked.set(true);
            Ok(())
        })
        .expect_err("wrong contract must reject");

    assert!(!invoked.get());
    assert_eq!(
        error,
        FrameError::ContractMismatch {
            expected: OrdinaryContract::BINDING.contract(),
            found: OtherContract::BINDING.contract(),
        }
    );
    assert_eq!(
        BoundExchangeFrame::<OrdinaryContract, RequestPayload, ReplyPayload>::decode(&bytes)
            .expect_err("bound frame decode must validate before rkyv"),
        error
    );
}

#[test]
fn wrong_revision_rejects_before_archive_decoder_is_invoked() {
    let header = ShortHeader::bound(NewerOrdinaryContract::BINDING, ROUTE);
    let mut bytes = header.to_le_bytes().to_vec();
    bytes.extend_from_slice(b"not an archive");
    let invoked = Cell::new(false);

    let error = OrdinaryContract::BINDING
        .decode_archive_with(&bytes, |_| {
            invoked.set(true);
            Ok(())
        })
        .expect_err("wrong revision must reject");

    assert!(!invoked.get());
    assert_eq!(
        error,
        FrameError::UnsupportedWireRevision {
            contract: OrdinaryContract::BINDING.contract(),
            expected: OrdinaryContract::BINDING.revision(),
            found: NewerOrdinaryContract::BINDING.revision(),
        }
    );
    assert_eq!(
        BoundExchangeFrame::<OrdinaryContract, RequestPayload, ReplyPayload>::decode(&bytes)
            .expect_err("bound frame decode must validate before rkyv"),
        error
    );
}

#[test]
fn bound_exchange_request_reply_and_control_preserve_identity() {
    let request_exchange = exchange(17);
    let request = BoundExchangeFrame::<OrdinaryContract, RequestPayload, ReplyPayload>::new(
        ROUTE,
        ExchangeFrameBody::Request {
            exchange: request_exchange,
            request: signal_frame::Request::from_payload(RequestPayload(41)),
        },
    );
    assert_eq!(request.short_header().binding(), OrdinaryContract::BINDING);
    let request_bytes = request.encode_length_prefixed().unwrap();
    let decoded_request =
        BoundExchangeFrame::<OrdinaryContract, RequestPayload, ReplyPayload>::decode_length_prefixed(
            &request_bytes,
        )
        .unwrap();
    let ExchangeFrameBody::Request {
        exchange: decoded_exchange,
        ..
    } = decoded_request.into_body()
    else {
        panic!("expected request");
    };
    assert_eq!(decoded_exchange, request_exchange);

    let reply = BoundExchangeFrame::<OrdinaryContract, RequestPayload, ReplyPayload>::new(
        ROUTE,
        ExchangeFrameBody::Reply {
            exchange: request_exchange,
            reply: signal_frame::Reply::rejected(signal_frame::RequestRejectionReason::Internal),
        },
    );
    assert_eq!(reply.short_header().binding(), OrdinaryContract::BINDING);

    let control = BoundExchangeFrame::<OrdinaryContract, RequestPayload, ReplyPayload>::new(
        ROUTE,
        ExchangeFrameBody::HandshakeRequest(HandshakeRequest::current()),
    );
    assert_eq!(control.short_header().binding(), OrdinaryContract::BINDING);
}

#[test]
fn binding_changes_only_the_header_not_the_archived_body() {
    let body = ExchangeFrameBody::HandshakeRequest(HandshakeRequest::current());
    let legacy = ExchangeFrame::<RequestPayload, ReplyPayload>::new(body.clone());
    let bound =
        BoundExchangeFrame::<OrdinaryContract, RequestPayload, ReplyPayload>::new(ROUTE, body);

    let legacy_archive = legacy.encode().unwrap();
    let bound_archive = bound.encode().unwrap();
    assert_ne!(&legacy_archive[..8], &bound_archive[..8]);
    assert_eq!(&legacy_archive[8..], &bound_archive[8..]);
}

#[test]
fn bound_stream_push_preserves_token_epoch_lane_sequence_and_binding() {
    let event_identifier = StreamEventIdentifier::new(
        SessionEpoch::new(23),
        ExchangeLane::Acceptor,
        LaneSequence::new(29),
    );
    let token = SubscriptionTokenInner::new(31);
    let frame =
        BoundStreamingFrame::<OrdinaryContract, RequestPayload, ReplyPayload, EventPayload>::new(
            ROUTE,
            StreamingFrameBody::SubscriptionEvent {
                event_identifier,
                token,
                event: EventPayload(37),
            },
        );

    assert_eq!(frame.short_header().binding(), OrdinaryContract::BINDING);
    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        BoundStreamingFrame::<OrdinaryContract, RequestPayload, ReplyPayload, EventPayload>::decode_length_prefixed(
            &bytes,
        )
        .unwrap();
    let StreamingFrameBody::SubscriptionEvent {
        event_identifier: decoded_identifier,
        token: decoded_token,
        ..
    } = decoded.into_body()
    else {
        panic!("expected event");
    };
    assert_eq!(decoded_identifier, event_identifier);
    assert_eq!(decoded_token, token);
}

#[test]
fn primitive_and_archived_layouts_are_stable_widths() {
    assert_eq!(size_of::<ContractId>(), 4);
    assert_eq!(size_of::<WireRevision>(), 2);
    assert_eq!(size_of::<RootCode>(), 1);
    assert_eq!(size_of::<VariantCode>(), 1);
    assert_eq!(size_of::<ShortHeader>(), 8);
    assert_eq!(size_of::<rkyv::Archived<ContractId>>(), 4);
    assert_eq!(size_of::<rkyv::Archived<WireRevision>>(), 2);
    assert_eq!(size_of::<rkyv::Archived<ContractBinding>>(), 6);
}
