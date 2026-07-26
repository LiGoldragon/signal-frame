use signal_frame::signal_channel;

pub struct Watch;
pub struct StopToken;
pub struct Opened;
pub struct Event;

signal_channel! {
    channel Terminal contract TestContract {
        operation Watch(Watch),
        operation Stop(StopToken),
    }
    reply TerminalReply {
        Opened(Opened),
    }
    event TerminalEvent {
        Event(Event) belongs Lifecycle,
    }
    stream Lifecycle {
        token StopToken;
        opened Opened;
        event Event;
        close Stop;
    }
}

fn main() {}
