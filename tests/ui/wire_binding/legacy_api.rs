use signal_frame::{LegacyExchangeFrame, LegacyStreamingFrame};

fn main() {
    let _ = core::mem::size_of::<LegacyExchangeFrame<(), ()>>();
    let _ = core::mem::size_of::<LegacyStreamingFrame<(), (), ()>>();
}
