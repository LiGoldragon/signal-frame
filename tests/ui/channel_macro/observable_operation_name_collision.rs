use signal_frame::signal_channel;

pub struct LedgerFilter;
pub struct Watch;
pub struct Note;
pub struct Acknowledgement;
pub struct OperationReceived;

signal_channel! {
    channel Ledger {
        operation Observe(Watch),
        operation Record(Note),
    }
    reply LedgerReply {
        Recorded(Acknowledgement),
    }
    observable {
        filter LedgerFilter;
        event OperationReceived;
    }
}

fn main() {}
