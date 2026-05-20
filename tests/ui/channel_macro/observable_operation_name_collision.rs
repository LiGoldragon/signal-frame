use signal_frame::signal_channel;

pub struct LedgerFilter;
pub struct Watch;
pub struct Note;
pub struct Acknowledgement;
pub struct OperationReceived;
pub struct SemaEffectEmitted;

signal_channel! {
    channel Ledger {
        operation Watch(Watch),
        operation Record(Note),
    }
    reply LedgerReply {
        Recorded(Acknowledgement),
    }
    observable {
        open Watch(LedgerFilter);
        close Unwatch;
        filter LedgerFilter;
        operation_event OperationReceived;
        effect_event SemaEffectEmitted;
    }
}

fn main() {}
