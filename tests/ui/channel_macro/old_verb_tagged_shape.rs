use signal_frame::signal_channel;

pub struct Submission;
pub struct Receipt;

signal_channel! {
    channel Message contract TestContract {
        Assert Submit(Submission),
    }
    reply MessageReply {
        Accepted(Receipt),
    }
}

fn main() {}
