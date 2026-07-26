use signal_frame::{ExchangeLane, LaneSequence, SessionEpoch, StreamEventIdentifier};

fn main() {
    let _ = StreamEventIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Connector,
        LaneSequence::first(),
    );
}
