use signal_frame::signal_channel;

pub struct LedgerFilter;
pub struct Note;
pub struct Acknowledgement;

// The observable block must declare both `operation_event` and
// `effect_event`; omitting either is a compile error. (The macro
// emits `publish_operation_received` and `publish_effect_emitted`
// over these two event roles.)
signal_channel! {
    channel Ledger contract TestContract {
        operation Record(Note),
    }
    reply LedgerReply {
        Recorded(Acknowledgement),
    }
    observable {
        filter LedgerFilter;
    }
}

fn main() {}
