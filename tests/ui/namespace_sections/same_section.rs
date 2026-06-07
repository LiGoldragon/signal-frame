mod ordinary_contract {
    pub const CONTRACT_SECTION: signal_frame::NamespaceSection = signal_frame::NamespaceSection::Big;
}

mod meta_contract {
    pub const CONTRACT_SECTION: signal_frame::NamespaceSection = signal_frame::NamespaceSection::Big;
}

signal_frame::assert_triad_sections!(ordinary_contract, meta_contract);

fn main() {}
