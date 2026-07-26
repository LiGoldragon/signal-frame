//! Async-first frame exchange + stream-event identity.
//!
//! The handshake establishes an [`ExchangeMode`]. Under `LaneSequence`,
//! each side owns its outbound lane (Connector / Acceptor — determined
//! by connection direction, not negotiated) and mints monotonic
//! [`LaneSequence`] values on it. Replies echo the originating
//! [`ExchangeIdentifier`]; subscription-event frames carry their own
//! [`StreamEventIdentifier`] on the acceptor lane.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};

/// Connection-instance epoch. A reconnect mints a fresh epoch so stale
/// in-flight replies from a previous connection cannot land on a new one.
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
pub struct SessionEpoch(u64);

impl SessionEpoch {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// Which endpoint of a connection minted a given frame. Determined by
/// connection direction at open time — not negotiated.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExchangeLane {
    /// The side that opened the connection.
    Connector,
    /// The side that accepted the connection.
    Acceptor,
}

/// Monotonic sequence within a single [`ExchangeLane`].
///
/// The same lane counter serves both request/reply exchanges and stream
/// events; identity is distinguished by the wrapping identifier type
/// ([`ExchangeIdentifier`] vs [`StreamEventIdentifier`]), not by
/// separate sequences.
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
pub struct LaneSequence(u64);

impl LaneSequence {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn first() -> Self {
        Self(0)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// Identifies a request/reply exchange. The request frame mints it; the
/// reply frame echoes it. The pair is the exchange.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExchangeIdentifier {
    session_epoch: SessionEpoch,
    lane: ExchangeLane,
    sequence: LaneSequence,
}

impl ExchangeIdentifier {
    pub const fn new(
        session_epoch: SessionEpoch,
        lane: ExchangeLane,
        sequence: LaneSequence,
    ) -> Self {
        Self {
            session_epoch,
            lane,
            sequence,
        }
    }

    pub const fn session_epoch(self) -> SessionEpoch {
        self.session_epoch
    }

    pub const fn lane(self) -> ExchangeLane {
        self.lane
    }

    pub const fn sequence(self) -> LaneSequence {
        self.sequence
    }
}

/// Identifies one subscription-event frame's position on the acceptor
/// lane. An event is a stream item, not half of a request/reply pair. The
/// Publishers, not this value type, own the monotonic counter and must pass
/// each successive sequence here.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamEventIdentifier {
    session_epoch: SessionEpoch,
    sequence: LaneSequence,
}

impl StreamEventIdentifier {
    pub const fn acceptor(session_epoch: SessionEpoch, sequence: LaneSequence) -> Self {
        Self {
            session_epoch,
            sequence,
        }
    }

    pub const fn session_epoch(self) -> SessionEpoch {
        self.session_epoch
    }

    pub const fn lane(self) -> ExchangeLane {
        ExchangeLane::Acceptor
    }

    pub const fn sequence(self) -> LaneSequence {
        self.sequence
    }
}

/// Exchange grammar a handshake establishes for the live connection.
/// The `LaneSequence` mode carries only `session_epoch` — lane
/// ownership is tautological from connection direction.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeMode {
    LaneSequence { session_epoch: SessionEpoch },
}

/// The handshake-time settlement of the exchange grammar.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExchangeHandshake {
    pub exchange_mode: ExchangeMode,
}

impl ExchangeHandshake {
    pub const fn lane_sequence(session_epoch: SessionEpoch) -> Self {
        Self {
            exchange_mode: ExchangeMode::LaneSequence { session_epoch },
        }
    }
}
