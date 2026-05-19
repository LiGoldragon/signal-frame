use signal_frame::signal_channel;

pub struct Watch;
pub struct StopPayload;
pub struct StreamToken;
pub struct Opened;
pub struct Event;

signal_channel! {
    channel Terminal {
        operation Watch(Watch) opens Lifecycle,
        operation Stop(StopPayload),
    }
    reply TerminalReply {
        Opened(Opened),
    }
    event TerminalEvent {
        Event(Event) belongs Lifecycle,
    }
    stream Lifecycle {
        token StreamToken;
        opened Opened;
        event Event;
        close Stop;
    }
}

fn main() {}
