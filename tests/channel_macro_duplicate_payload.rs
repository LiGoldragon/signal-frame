use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_frame::{
    ContractBinding, ContractId, ExchangeFrameBody, ExchangeIdentifier, ExchangeLane, LaneSequence,
    NonEmpty, RootCode, SessionEpoch, SubReply, VariantCode, WireContract, WireRevision, WireRoute,
    signal_channel,
};
use std::num::{NonZeroU16, NonZeroU32};

#[derive(Debug, PartialEq, Eq)]
struct DuplicatePayloadContract;

impl WireContract for DuplicatePayloadContract {
    const BINDING: ContractBinding = ContractBinding::new(
        ContractId::new(NonZeroU32::MIN),
        WireRevision::new(NonZeroU16::MIN),
    );
}

#[cfg_attr(feature = "dotos-text", derive(dotos::DotosEncode, dotos::DotosDecode))]
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct Command(String);

#[cfg_attr(feature = "dotos-text", derive(dotos::DotosEncode, dotos::DotosDecode))]
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct Snapshot(String);

mod contract {
    use super::*;

    signal_channel! {
        channel DuplicatePayload contract DuplicatePayloadContract {
            operation Submit(Command),
        }
        reply Reply {
            Accepted(Snapshot),
            Released(Snapshot),
        }
    }
}

#[test]
fn binary_channel_accepts_distinct_reply_variants_with_the_same_payload_type() {
    let exchange = ExchangeIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Connector,
        LaneSequence::first(),
    );
    let accepted = contract::Frame::new(
        WireRoute::new(RootCode::new(0), VariantCode::new(0)),
        ExchangeFrameBody::Reply {
            exchange,
            reply: signal_frame::Reply::committed(NonEmpty::single(SubReply::Ok(
                contract::Reply::Accepted(Snapshot("one".into())),
            ))),
        },
    );
    let released = contract::Frame::new(
        WireRoute::new(RootCode::new(0), VariantCode::new(1)),
        ExchangeFrameBody::Reply {
            exchange,
            reply: signal_frame::Reply::committed(NonEmpty::single(SubReply::Ok(
                contract::Reply::Released(Snapshot("two".into())),
            ))),
        },
    );

    for frame in [accepted, released] {
        let bytes = frame.encode().expect("encode reply frame");
        assert_eq!(
            contract::Frame::decode(&bytes).expect("decode reply frame"),
            frame
        );
    }
}
