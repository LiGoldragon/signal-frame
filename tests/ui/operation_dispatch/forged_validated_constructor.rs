use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_frame::signal_channel;

#[cfg_attr(feature = "nota-text", derive(nota::NotaEncode, nota::NotaDecode))]
#[derive(
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
pub struct Submission {
    value: u8,
}

#[cfg_attr(feature = "nota-text", derive(nota::NotaEncode, nota::NotaDecode))]
#[derive(
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
    Debug,
    Clone,
    PartialEq,
    Eq,
)]
pub struct Receipt {
    value: u8,
}

struct TestContract;

impl signal_frame::WireContract for TestContract {
    const BINDING: signal_frame::ContractBinding = signal_frame::ContractBinding::new(
        signal_frame::ContractId::new(core::num::NonZeroU32::MIN),
        signal_frame::WireRevision::new(core::num::NonZeroU16::MIN),
    );
}

mod message {
    use super::*;

    signal_channel! {
        channel Message contract TestContract {
            operation Submit(Submission),
        }
        reply Reply {
            Accepted(Receipt),
        }
    }
}

fn main() {
    let operation = message::Operation::Submit(Submission { value: 0 });
    let _ = message::ValidatedOperation { operation };
}
