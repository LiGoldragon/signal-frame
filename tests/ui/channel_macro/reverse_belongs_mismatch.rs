use signal_frame::signal_channel;

pub struct Watch;
pub struct WatchOther;
pub struct StopToken;
pub struct Opened;
pub struct Event;

signal_channel! {
    channel Terminal {
        operation Watch(Watch) opens Lifecycle,
        operation WatchOther(WatchOther) opens OtherLifecycle,
        operation Stop(StopToken),
    }
    reply TerminalReply {
        Opened(Opened),
    }
    event TerminalEvent {
        Event(Event) belongs OtherLifecycle,
    }
    stream Lifecycle {
        token StopToken;
        opened Opened;
        event Event;
        close Stop;
    }
    stream OtherLifecycle {
        token StopToken;
        opened Opened;
        event Event;
        close Stop;
    }
}

fn main() {}
