#[test]
fn triad_section_assertion_rejects_collisions() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/namespace_sections/*.rs");
}
