use signal_frame::signal_channel;

pub struct LedgerFilter;
pub struct Note;
pub struct Acknowledgement;
pub struct OperationReceived;

signal_channel! {
    channel Ledger {
        operation Record(Note),
    }
    reply LedgerReply {
        Recorded(Acknowledgement),
    }
    observable {
        filter LedgerFilter;
        event OperationReceived;
    }
    observable {
        filter LedgerFilter;
        event OperationReceived;
    }
}

fn main() {}
