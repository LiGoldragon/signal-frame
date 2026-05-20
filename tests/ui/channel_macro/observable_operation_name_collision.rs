use signal_frame::signal_channel;

pub struct LedgerFilter;
pub struct TapPayload;
pub struct Note;
pub struct Acknowledgement;
pub struct OperationReceived;
pub struct EffectEmitted;

// Per /246 §2: the observable block injects fixed `Tap` / `Untap`
// operations. A contract that declares `Tap` (or `Untap`) as a domain
// operation collides — the contract author must rename the domain
// verb (the observability verbs are workspace-uniform; not negotiable).
signal_channel! {
    channel Ledger {
        operation Tap(TapPayload),
        operation Record(Note),
    }
    reply LedgerReply {
        Recorded(Acknowledgement),
    }
    observable {
        filter LedgerFilter;
        operation_event OperationReceived;
        effect_event EffectEmitted;
    }
}

fn main() {}
