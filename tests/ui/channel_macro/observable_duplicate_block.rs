use signal_frame::signal_channel;

pub struct LedgerFilter;
pub struct Note;
pub struct Acknowledgement;
pub struct OperationReceived;
pub struct SemaEffectEmitted;

// Only one observable block per channel; duplicates are a compile
// error.
signal_channel! {
    channel Ledger {
        operation Record(Note),
    }
    reply LedgerReply {
        Recorded(Acknowledgement),
    }
    observable {
        filter LedgerFilter;
        operation_event OperationReceived;
        effect_event SemaEffectEmitted;
    }
    observable {
        filter LedgerFilter;
        operation_event OperationReceived;
        effect_event SemaEffectEmitted;
    }
}

fn main() {}
