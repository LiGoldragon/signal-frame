//! Frame-bound identity primitives.
//!
//! `Slot<T>` is a typed slot number — a wire identity for a row in a
//! receiver's typed state. `Revision` is the per-row revision counter
//! used by pre-condition checks. Both are wire records here;
//! allocation, lookup, revision bumping, and persistence belong to the
//! Sema engine.

use std::marker::PhantomData;

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};

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
pub struct Slot<Payload> {
    number: u64,
    payload: PhantomData<Payload>,
}

impl<Payload> Slot<Payload> {
    pub const fn new(number: u64) -> Self {
        Self {
            number,
            payload: PhantomData,
        }
    }

    pub const fn number(&self) -> u64 {
        self.number
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
pub struct Revision {
    number: u64,
}

impl Revision {
    pub const fn new(number: u64) -> Self {
        Self { number }
    }

    pub const fn initial() -> Self {
        Self::new(0)
    }

    pub const fn number(&self) -> u64 {
        self.number
    }
}
