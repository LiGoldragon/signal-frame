use signal_frame::signal_channel;

mod first {
    pub struct Status;
}

mod second {
    pub struct Status;
}

pub struct Receipt;

signal_channel! {
    channel StatusChannel {
        operation First(first::Status),
        operation Second(second::Status),
    }
    reply StatusReply {
        Accepted(Receipt),
    }
}

fn main() {}
