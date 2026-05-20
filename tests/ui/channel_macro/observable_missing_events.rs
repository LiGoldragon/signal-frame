use signal_frame::signal_channel;

pub struct LedgerFilter;
pub struct Note;
pub struct Acknowledgement;

signal_channel! {
    channel Ledger {
        operation Record(Note),
    }
    reply LedgerReply {
        Recorded(Acknowledgement),
    }
    observable {
        open Watch(LedgerFilter);
        close Unwatch;
        filter LedgerFilter;
    }
}

fn main() {}
