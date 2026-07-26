use std::num::{NonZeroU16, NonZeroU32};
use thiserror::Error;

use crate::error::FrameError;

/// Stable numeric identity allocated to one wire contract.
///
/// Zero is reserved for explicitly parsed legacy frames. Production
/// contracts construct IDs through [`ContractId::new`].
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContractId(NonZeroU32);

impl ContractId {
    pub const fn new(value: NonZeroU32) -> Self {
        Self(value)
    }

    pub const fn try_new(value: u32) -> Result<Self, BindingIdentifierError> {
        match NonZeroU32::new(value) {
            Some(value) => Ok(Self(value)),
            None => Err(BindingIdentifierError::ReservedLegacyContractId),
        }
    }

    pub const fn value(self) -> u32 {
        self.0.get()
    }
}

/// Contract-local revision of the archived wire body.
///
/// Zero is reserved for explicitly parsed legacy frames. Production
/// contracts construct revisions through [`WireRevision::new`].
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WireRevision(NonZeroU16);

impl WireRevision {
    pub const fn new(value: NonZeroU16) -> Self {
        Self(value)
    }

    pub const fn try_new(value: u16) -> Result<Self, BindingIdentifierError> {
        match NonZeroU16::new(value) {
            Some(value) => Ok(Self(value)),
            None => Err(BindingIdentifierError::ReservedLegacyWireRevision),
        }
    }

    pub const fn value(self) -> u16 {
        self.0.get()
    }
}

/// Contract identity paired with the exact archived-body revision it accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContractBinding {
    contract: ContractId,
    revision: WireRevision,
}

impl ContractBinding {
    pub const fn new(contract: ContractId, revision: WireRevision) -> Self {
        Self { contract, revision }
    }

    pub const fn contract(self) -> ContractId {
        self.contract
    }

    pub const fn revision(self) -> WireRevision {
        self.revision
    }

    pub fn validate_header(self, header: crate::ShortHeader) -> Result<(), FrameError> {
        let found = header.binding()?;
        if found.contract != self.contract {
            return Err(FrameError::ContractMismatch {
                expected: self.contract,
                found: found.contract,
            });
        }
        if found.revision != self.revision {
            return Err(FrameError::UnsupportedWireRevision {
                contract: self.contract,
                expected: self.revision,
                found: found.revision,
            });
        }
        Ok(())
    }
}

/// Route root byte. Its meaning remains local to the bound contract.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RootCode(u8);

impl RootCode {
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u8 {
        self.0
    }
}

/// Route variant byte. Its meaning remains local to the bound contract.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VariantCode(u8);

impl VariantCode {
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u8 {
        self.0
    }
}

/// Contract-local route carried in the high sixteen bits of a short header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WireRoute {
    root: RootCode,
    variant: VariantCode,
}

impl WireRoute {
    pub const fn new(root: RootCode, variant: VariantCode) -> Self {
        Self { root, variant }
    }

    pub const fn from_log_variant(value: u64) -> Self {
        Self {
            root: RootCode::new(value as u8),
            variant: VariantCode::new((value >> 8) as u8),
        }
    }

    pub const fn root(self) -> RootCode {
        self.root
    }

    pub const fn variant(self) -> VariantCode {
        self.variant
    }
}

/// Type-level seam binding a consumer's frame constructors to one contract.
///
/// Allocation remains outside this crate. Contract crates implement the trait
/// with constants supplied by the workspace allocation owner.
pub trait WireContract {
    const BINDING: ContractBinding;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BindingIdentifierError {
    #[error("contract id zero is reserved for explicit legacy parsing")]
    ReservedLegacyContractId,
    #[error("wire revision zero is reserved for explicit legacy parsing")]
    ReservedLegacyWireRevision,
}
