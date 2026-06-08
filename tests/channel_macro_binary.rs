#![cfg(not(feature = "nota-text"))]

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_frame::{
    ExchangeIdentifier, ExchangeLane, LaneSequence, NonEmpty, Reply as FrameReply, RequestPayload,
    SessionEpoch, SubReply, signal_channel,
};

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct Ping {
    pub sequence: u64,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct Pong {
    pub sequence: u64,
}

signal_channel! {
    channel BinaryOnly {
        operation Ping(Ping),
    }
    reply Reply {
        Pong(Pong),
    }
}

#[test]
fn signal_channel_macro_does_not_require_nota_text_in_default_build() {
    let operation = Operation::Ping(Ping { sequence: 7 });
    let frame = Frame::new(FrameBody::Request {
        exchange: ExchangeIdentifier::new(
            SessionEpoch::new(1),
            ExchangeLane::Connector,
            LaneSequence::first(),
        ),
        request: operation.clone().into_request(),
    });

    let bytes = frame.encode_length_prefixed().expect("encode request");
    let decoded = Frame::decode_length_prefixed(&bytes).expect("decode");

    match decoded.into_body() {
        FrameBody::Request { request, .. } => {
            assert_eq!(request.payloads().head(), &operation);
        }
        other => panic!("expected request, got {other:?}"),
    }

    let reply = Reply::Pong(Pong { sequence: 7 });
    let frame = Frame::new(FrameBody::Reply {
        exchange: ExchangeIdentifier::new(
            SessionEpoch::new(1),
            ExchangeLane::Connector,
            LaneSequence::first(),
        ),
        reply: FrameReply::committed(NonEmpty::single(SubReply::Ok(reply.clone()))),
    });

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
