use signal_frame::signal_channel;

pub struct Note;
pub struct Acknowledgement;
pub struct OperationReceived;
pub struct SemaEffectEmitted;

// The observable block must open with `filter <Type>;` (or
// `filter default;`); omitting the filter line is a compile error.
signal_channel! {
    channel Ledger {
        operation Record(Note),
    }
    reply LedgerReply {
        Recorded(Acknowledgement),
    }
    observable {
        operation_event OperationReceived;
        effect_event SemaEffectEmitted;
    }
}

fn main() {}
