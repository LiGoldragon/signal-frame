//! Subscription identity.
//!
//! `signal-frame` provides only the opaque `u64`-backed inner token. Each
//! subscribe-bearing channel declares its own typed newtype around
//! [`SubscriptionTokenInner`] so subscription retraction is channel-typed
//! and cross-channel confusion is unrepresentable at the type level.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionTokenInner(u64);

impl SubscriptionTokenInner {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}
