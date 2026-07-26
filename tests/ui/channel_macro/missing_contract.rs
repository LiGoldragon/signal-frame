use signal_frame::signal_channel;

signal_channel! {
    channel MissingContract {
        operation Submit(Submission),
    }
    reply Reply { Accepted }
}

struct Submission;

fn main() {}
