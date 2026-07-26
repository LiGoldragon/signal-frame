use signal_frame::{ContractBinding, WireContract};

struct UnboundContract;

impl WireContract for UnboundContract {
    const BINDING: ContractBinding = ContractBinding::legacy_unbound();
}

fn main() {}
