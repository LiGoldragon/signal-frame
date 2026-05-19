use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolVersion {
    major: u16,
    minor: u16,
    patch: u16,
}

impl ProtocolVersion {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub const fn major(&self) -> u16 {
        self.major
    }

    pub const fn minor(&self) -> u16 {
        self.minor
    }

    pub const fn patch(&self) -> u16 {
        self.patch
    }

    pub const fn accepts(&self, peer: Self) -> bool {
        self.major == peer.major && self.minor >= peer.minor
    }
}

pub const SIGNAL_FRAME_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::new(0, 1, 0);

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct HandshakeRequest {
    version: ProtocolVersion,
}

impl HandshakeRequest {
    pub fn new(version: ProtocolVersion) -> Self {
        Self { version }
    }

    pub fn current() -> Self {
        Self::new(SIGNAL_FRAME_PROTOCOL_VERSION)
    }

    pub fn version(&self) -> ProtocolVersion {
        self.version
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum HandshakeReply {
    Accepted(ProtocolVersion),
    Rejected(HandshakeRejectionReason),
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum HandshakeRejectionReason {
    IncompatibleVersion {
        local: ProtocolVersion,
        peer: ProtocolVersion,
    },
}
