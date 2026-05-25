use signal_frame::{LogVariant, ShortHeader};

signal_frame::emit_schema!("schema-rust/tests/fixtures/simple.schema");

#[test]
fn emit_schema_generates_route_table_from_schema_header() {
    assert_eq!(simple::ROUTE_COUNT, 3);

    let route = simple::route_for_short_header(
        simple::Leg::Ordinary,
        ShortHeader::new(u64::from_le_bytes([0, 1, 0, 0, 0, 0, 0, 0])),
    )
    .expect("state declaration route");

    assert_eq!(route.root, "State");
    assert_eq!(route.endpoint, "Declaration");
    assert_eq!(route.body, simple::RouteBodyDescriptor::Type("Declaration"));
}

#[test]
fn emit_schema_generates_short_header_projection_from_schema_slots() {
    let topic = simple::Topic("workspace".to_string());
    let text = simple::Text("schema drives the operation tree".to_string());
    let operation =
        simple::Operation::State(simple::StateEndpoint::Declaration(simple::Declaration {
            topic,
            kind: simple::Kind::Decision,
            text,
        }));

    assert_eq!(
        operation.log_variant(),
        u64::from_le_bytes([0, 1, 0, 0, 0, 0, 0, 0])
    );
}

#[test]
fn emit_schema_generates_positional_struct_fields_from_type_names() {
    let entry = simple::Entry {
        topic: simple::Topic("schema".to_string()),
        kind: simple::Kind::Principle,
        summary: simple::Summary("field names come from type names".to_string()),
        context: simple::Context("schema records are positional".to_string()),
        quote: simple::Quote("we derive the name from the name of the type".to_string()),
    };

    assert_eq!(entry.topic.0, "schema");
    assert_eq!(entry.kind, simple::Kind::Principle);
}
