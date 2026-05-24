//! Tier-1 short-header projection.
//!
//! Channel operation enums implement [`LogVariant`] so frame construction
//! can populate the mandatory 64-bit short header before the archived
//! body.

/// Project a typed value into the 64-bit short-header layout.
///
/// Byte 0 carries the root enum discriminator. Bytes 1-7 carry the
/// first seven sized sub-enum discriminators found in positional order.
// DESIGN-DECISION-REVIEW (designer/320 §2.12): LogVariant trait in
// signal-frame crate root. Alternative: separate signal-log-variant
// crate. Revisit if the trait grows many impls that do not naturally
// live with signal-frame.
pub trait LogVariant {
    fn log_variant(&self) -> u64;
}
