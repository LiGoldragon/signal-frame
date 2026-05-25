#[test]
fn channel_macro_rejects_invalid_contract_local_shapes() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/channel_macro/close_payload_mismatch.rs");
    tests.compile_fail("tests/ui/channel_macro/duplicate_record_head.rs");
    tests.compile_fail("tests/ui/channel_macro/observable_duplicate_block.rs");
    tests.compile_fail("tests/ui/channel_macro/observable_missing_events.rs");
    tests.compile_fail("tests/ui/channel_macro/observable_missing_filter.rs");
    tests.compile_fail("tests/ui/channel_macro/observable_operation_name_collision.rs");
    tests.compile_fail("tests/ui/channel_macro/old_verb_tagged_shape.rs");
    tests.compile_fail("tests/ui/channel_macro/orphan_stream.rs");
    tests.compile_fail("tests/ui/channel_macro/reverse_belongs_mismatch.rs");
}
