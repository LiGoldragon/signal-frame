use signal_frame::{NamespaceSection, SECTION_CUTOFF, namespace};

mod ordinary_contract {
    pub const CONTRACT_SECTION: signal_frame::NamespaceSection =
        signal_frame::NamespaceSection::Big;
}

mod meta_contract {
    pub const CONTRACT_SECTION: signal_frame::NamespaceSection =
        signal_frame::NamespaceSection::Small;
}

signal_frame::assert_triad_sections!(ordinary_contract, meta_contract);

#[test]
fn section_cutoff_is_decimal_one_hundred() {
    assert_eq!(SECTION_CUTOFF, 100);
}

#[test]
fn classify_maps_small_section_to_lower_hundred_values() {
    for byte in 0x00..SECTION_CUTOFF {
        assert_eq!(namespace::classify(byte), NamespaceSection::Small);
        assert_eq!(NamespaceSection::classify(byte), NamespaceSection::Small);
    }
}

#[test]
fn classify_maps_big_section_to_upper_values() {
    for byte in SECTION_CUTOFF..=u8::MAX {
        assert_eq!(namespace::classify(byte), NamespaceSection::Big);
        assert_eq!(NamespaceSection::classify(byte), NamespaceSection::Big);
    }
}
