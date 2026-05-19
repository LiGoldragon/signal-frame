#[test]
fn channel_macro_rejects_invalid_contract_local_shapes() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/channel_macro/*.rs");
}
