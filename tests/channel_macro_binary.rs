#![cfg(not(feature = "dotos-text"))]

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_frame::{
    ContractBinding, ContractId, ExchangeIdentifier, ExchangeLane, LaneSequence, NonEmpty,
    Reply as FrameReply, RequestPayload, SessionEpoch, SubReply, WireContract, WireRevision,
    signal_channel,
};
use std::num::{NonZeroU16, NonZeroU32};

struct TestContract;

impl WireContract for TestContract {
    const BINDING: ContractBinding = ContractBinding::new(
        ContractId::new(NonZeroU32::MIN),
        WireRevision::new(NonZeroU16::MIN),
    );
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct Ping {
    pub sequence: u64,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct Pong {
    pub sequence: u64,
}

signal_channel! {
    channel BinaryOnly contract TestContract {
        operation Ping(Ping),
    }
    reply Reply {
        Pong(Pong),
    }
}

#[test]
fn signal_channel_macro_does_not_require_dotos_text_in_default_build() {
    let operation = Operation::Ping(Ping { sequence: 7 });
    let frame = Frame::new(
        operation.clone().into_request().route().unwrap(),
        FrameBody::Request {
            exchange: ExchangeIdentifier::new(
                SessionEpoch::new(1),
                ExchangeLane::Connector,
                LaneSequence::first(),
            ),
            request: operation.clone().into_request(),
        },
    );
    let frame: signal_frame::BoundExchangeFrame<TestContract, Operation, Reply> = frame;

    let bytes = frame.encode_length_prefixed().expect("encode request");
    let decoded = Frame::decode_length_prefixed(&bytes).expect("decode");

    match decoded.into_body() {
        FrameBody::Request { request, .. } => {
            assert_eq!(request.payloads().head(), &operation);
        }
        other => panic!("expected request, got {other:?}"),
    }

    let reply = Reply::Pong(Pong { sequence: 7 });
    let frame = Frame::new(
        signal_frame::WireRoute::new(
            signal_frame::RootCode::new(0),
            signal_frame::VariantCode::new(0),
        ),
        FrameBody::Reply {
            exchange: ExchangeIdentifier::new(
                SessionEpoch::new(1),
                ExchangeLane::Connector,
                LaneSequence::first(),
            ),
            reply: FrameReply::committed(NonEmpty::single(SubReply::Ok(reply.clone()))),
        },
    );

    let bytes = frame.encode_length_prefixed().expect("encode reply");
    let decoded = Frame::decode_length_prefixed(&bytes).expect("decode");

    match decoded.into_body() {
        FrameBody::Reply {
            reply: envelope, ..
        } => match envelope {
            FrameReply::Accepted { per_operation, .. } => {
                assert_eq!(per_operation.head(), &SubReply::Ok(reply));
            }
            other => panic!("expected accepted reply, got {other:?}"),
        },
        other => panic!("expected reply, got {other:?}"),
    }
}
