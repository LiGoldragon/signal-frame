//! Golden-ratio sections for the byte-0 root-verb namespace.
//!
//! A component triad's ordinary and owner contracts divide the 256
//! possible byte-0 root-verb discriminators into two asymmetric sections:
//! `Small` is `0x00..=0x63` and `Big` is `0x64..=0xFF`.
//!
//! The cutoff is decimal `100` (`0x64`), the first byte in the `Big`
//! section. Decimal 100 is intentionally human-memorable for audits and
//! debugging. Power-of-two alternatives such as 64/192 or 128/128 are not
//! used because the asymmetry is the point: ordinary contracts usually have
//! more public operation verbs than owner contracts have policy verbs.

/// Section of the byte-0 root-verb namespace claimed by a contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceSection {
    /// The small section, covering `0x00..=0x63` (decimal `0..=99`).
    Small,

    /// The big section, covering `0x64..=0xFF` (decimal `100..=255`).
    Big,
}

/// First byte value in the [`NamespaceSection::Big`] section.
///
/// Values less than `SECTION_CUTOFF` classify as [`NamespaceSection::Small`];
/// values greater than or equal classify as [`NamespaceSection::Big`]. The
/// decimal value `100` is deliberate: it is easy to remember during human
/// inspection while preserving the intended golden-ratio asymmetry.
pub const SECTION_CUTOFF: u8 = 100;

/// Classify a byte-0 root-verb discriminator into its namespace section.
pub const fn classify(byte: u8) -> NamespaceSection {
    if byte < SECTION_CUTOFF {
        NamespaceSection::Small
    } else {
        NamespaceSection::Big
    }
}

impl NamespaceSection {
    /// Classify a byte-0 root-verb discriminator into its namespace section.
    pub const fn classify(byte: u8) -> Self {
        classify(byte)
    }
}

/// Assert at daemon compile time that paired ordinary and owner contracts claim
/// opposite byte-0 namespace sections.
///
/// Each path must name a crate or module that exports
/// `CONTRACT_SECTION: signal_frame::NamespaceSection`. The daemon crate is the
/// witness site because it depends on both contracts and is where the two
/// byte-0 spaces must coexist.
#[macro_export]
macro_rules! assert_triad_sections {
    ($ordinary:path, $owner:path $(,)?) => {
        const _: () = {
            use $ordinary as __signal_frame_ordinary_contract;
            use $owner as __signal_frame_owner_contract;

            match (
                __signal_frame_ordinary_contract::CONTRACT_SECTION,
                __signal_frame_owner_contract::CONTRACT_SECTION,
            ) {
                ($crate::NamespaceSection::Small, $crate::NamespaceSection::Big)
                | ($crate::NamespaceSection::Big, $crate::NamespaceSection::Small) => {}
                ($crate::NamespaceSection::Small, $crate::NamespaceSection::Small) => {
                    panic!(concat!(
                        stringify!($ordinary),
                        " and ",
                        stringify!($owner),
                        " both claim NamespaceSection::Small; ordinary and owner contracts must claim opposite byte-0 namespace sections"
                    ));
                }
                ($crate::NamespaceSection::Big, $crate::NamespaceSection::Big) => {
                    panic!(concat!(
                        stringify!($ordinary),
                        " and ",
                        stringify!($owner),
                        " both claim NamespaceSection::Big; ordinary and owner contracts must claim opposite byte-0 namespace sections"
                    ));
                }
            }
        };
    };
}
