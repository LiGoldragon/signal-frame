use std::{
    fs,
    path::{Path, PathBuf},
};

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CallerIdentity(String);

impl CallerIdentity {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
pub struct ProcessIdentifier(u32);

impl ProcessIdentifier {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u32 {
        self.0
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExecutablePath(String);

impl ExecutablePath {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into().to_string_lossy().into_owned())
    }

    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessStartTime {
    clock_ticks_after_boot: u64,
}

impl ProcessStartTime {
    pub const fn new(clock_ticks_after_boot: u64) -> Self {
        Self {
            clock_ticks_after_boot,
        }
    }

    pub const fn clock_ticks_after_boot(self) -> u64 {
        self.clock_ticks_after_boot
    }
}

/// Advisory caller context captured or supplied by a thin CLI before it sends a frame.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub identity: Option<CallerIdentity>,
    pub pid: ProcessIdentifier,
    pub executable: Option<ExecutablePath>,
    pub start_time: Option<ProcessStartTime>,
}

impl Caller {
    pub fn new(
        pid: ProcessIdentifier,
        executable: Option<ExecutablePath>,
        start_time: Option<ProcessStartTime>,
    ) -> Self {
        Self {
            identity: None,
            pid,
            executable,
            start_time,
        }
    }

    pub fn current_process() -> Self {
        let process_identifier = std::process::id();
        Self {
            identity: None,
            pid: ProcessIdentifier::new(process_identifier),
            executable: std::env::current_exe().ok().map(ExecutablePath::new),
            start_time: process_start_time(process_identifier).map(ProcessStartTime::new),
        }
    }

    pub fn identity(&self) -> Option<&CallerIdentity> {
        self.identity.as_ref()
    }

    pub fn with_identity(mut self, identity: Option<CallerIdentity>) -> Self {
        self.identity = identity;
        self
    }

    /// Capture the parent process context for a CLI invocation.
    ///
    /// The data is advisory. Socket credentials remain the authoritative
    /// security boundary in the daemon.
    pub fn from_kernel() -> Option<Self> {
        let parent = std::os::unix::process::parent_id();
        if parent == 1 {
            return None;
        }
        Some(Self {
            identity: None,
            pid: ProcessIdentifier::new(parent),
            executable: parent_executable(parent).map(ExecutablePath::new),
            start_time: process_start_time(parent).map(ProcessStartTime::new),
        })
    }
}

fn parent_executable(parent: u32) -> Option<PathBuf> {
    fs::read_link(format!("/proc/{parent}/exe")).ok()
}

fn process_start_time(process_identifier: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{process_identifier}/stat")).ok()?;
    start_time_ticks(&stat)
}

fn start_time_ticks(stat: &str) -> Option<u64> {
    let close = stat.rfind(") ")?;
    let fields: Vec<&str> = stat[close + 2..].split_whitespace().collect();
    fields.get(19)?.parse().ok()
}
