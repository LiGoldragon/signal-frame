#[test]
fn production_contract_cannot_select_the_reserved_legacy_binding() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/wire_binding/unbound_contract.rs");
}
