use nota_codec::{NotaDecode, NotaEncode, NotaRecord};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_frame::{
    ExchangeFrame, ExchangeFrameBody, ExchangeIdentifier, ExchangeLane, LaneSequence, NonEmpty,
    ObservableSet, Request, RequestPayload, SessionEpoch, StreamingFrame, StreamingFrameBody,
    SubReply, signal_channel,
};

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    body: String,
}

impl Submission {
    fn new(body: impl Into<String>) -> Self {
        Self { body: body.into() }
    }
}

impl NotaEncode for Submission {
    fn encode(&self, encoder: &mut nota_codec::Encoder) -> nota_codec::Result<()> {
        encoder.start_record("Submission")?;
        self.body.encode(encoder)?;
        encoder.end_record()
    }
}

impl NotaDecode for Submission {
    fn decode(decoder: &mut nota_codec::Decoder<'_>) -> nota_codec::Result<Self> {
        decoder.expect_record_head("Submission")?;
        let body = String::decode(decoder)?;
        decoder.expect_record_end()?;
        Ok(Self { body })
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct InboxQuery {
    name: String,
}

impl InboxQuery {
    fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl NotaEncode for InboxQuery {
    fn encode(&self, encoder: &mut nota_codec::Encoder) -> nota_codec::Result<()> {
        encoder.start_record("InboxQuery")?;
        self.name.encode(encoder)?;
        encoder.end_record()
    }
}

impl NotaDecode for InboxQuery {
    fn decode(decoder: &mut nota_codec::Decoder<'_>) -> nota_codec::Result<Self> {
        decoder.expect_record_head("InboxQuery")?;
        let name = String::decode(decoder)?;
        decoder.expect_record_end()?;
        Ok(Self { name })
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    accepted: bool,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct Inbox {
    count: u64,
}

signal_channel! {
    channel Message {
        operation Submit(Submission),
        operation Query(InboxQuery),
    }
    reply MessageReply {
        Accepted(Receipt),
        Inbox(Inbox),
    }
}

fn fresh_exchange() -> ExchangeIdentifier {
    ExchangeIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Connector,
        LaneSequence::first(),
    )
}

fn encode_to_text<T: NotaEncode>(value: &T) -> String {
    let mut encoder = nota_codec::Encoder::new();
    value.encode(&mut encoder).expect("encode");
    encoder.into_string()
}

#[test]
fn macro_emits_contract_local_operation_enum_without_signal_verb() {
    let operation = MessageOperation::Submit(Submission::new("hello"));

    assert_eq!(operation.kind(), MessageOperationKind::Submit);

    let request = operation.into_request();
    assert_eq!(request.payloads().len(), 1);
    assert_eq!(encode_to_text(&request), "(Submit (Submission hello))");
}

#[test]
fn macro_request_text_round_trips_through_contract_local_heads() {
    let request = Request::from_payloads(NonEmpty::from_head_and_tail(
        MessageOperation::Submit(Submission::new("first")),
        vec![MessageOperation::Query(InboxQuery::new("operator"))],
    ));
    let text = encode_to_text(&request);

    assert_eq!(
        text,
        "[(Submit (Submission first)) (Query (InboxQuery operator))]"
    );

    let mut decoder = nota_codec::Decoder::new(&text);
    let decoded = Request::<MessageOperation>::decode(&mut decoder).expect("decode");
    assert_eq!(decoded, request);
}

#[test]
fn macro_frame_alias_round_trips_with_generated_payloads() {
    let exchange = fresh_exchange();
    let request = MessageOperation::Submit(Submission::new("frame")).into_request();
    let frame = MessageFrame::new(ExchangeFrameBody::Request {
        exchange,
        request: request.clone(),
    });

    let bytes = frame.encode_length_prefixed().expect("encode");
    let decoded = ExchangeFrame::<MessageOperation, MessageReply>::decode_length_prefixed(&bytes)
        .expect("decode");

    match decoded.into_body() {
        ExchangeFrameBody::Request {
            exchange: decoded_exchange,
            request: decoded_request,
        } => {
            assert_eq!(decoded_exchange, exchange);
            assert_eq!(decoded_request, request);
        }
        _ => panic!("expected request frame"),
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct WatchWorker {
    name: String,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct WorkerToken {
    number: u64,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct WorkerOpened {
    number: u64,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct WorkerStopped {
    number: u64,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct WorkerStarted {
    number: u64,
}

signal_channel! {
    channel Terminal {
        operation Watch(WatchWorker) opens WorkerLifecycle,
        operation Stop(WorkerToken),
    }
    reply TerminalReply {
        Opened(WorkerOpened),
        Stopped(WorkerStopped),
    }
    event TerminalEvent {
        Started(WorkerStarted) belongs WorkerLifecycle,
    }
    stream WorkerLifecycle {
        token WorkerToken;
        opened Opened;
        event Started;
        close Stop;
    }
}

#[test]
fn macro_stream_witnesses_are_contract_local_not_subscribe_retract_bound() {
    let watch = TerminalOperation::Watch(WatchWorker {
        name: "worker".into(),
    });
    let stop = TerminalOperation::Stop(WorkerToken { number: 7 });
    let event = TerminalEvent::Started(WorkerStarted { number: 7 });

    assert_eq!(
        watch.opened_stream(),
        Some(TerminalStreamKind::WorkerLifecycle)
    );
    assert_eq!(
        stop.closed_stream(),
        Some(TerminalStreamKind::WorkerLifecycle)
    );
    assert_eq!(event.stream_kind(), TerminalStreamKind::WorkerLifecycle);
}

#[test]
fn macro_streaming_frame_alias_round_trips() {
    let event_identifier = signal_frame::StreamEventIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Acceptor,
        LaneSequence::first(),
    );
    let frame = TerminalFrame::new(StreamingFrameBody::SubscriptionEvent {
        event_identifier,
        token: signal_frame::SubscriptionTokenInner::new(5),
        event: TerminalEvent::Started(WorkerStarted { number: 5 }),
    });

    let bytes = frame.encode_length_prefixed().expect("encode");
    let decoded =
        StreamingFrame::<TerminalOperation, TerminalReply, TerminalEvent>::decode_length_prefixed(
            &bytes,
        )
        .expect("decode");

    match decoded.into_body() {
        StreamingFrameBody::SubscriptionEvent {
            event_identifier: decoded_identifier,
            token,
            event,
        } => {
            assert_eq!(decoded_identifier, event_identifier);
            assert_eq!(token, signal_frame::SubscriptionTokenInner::new(5));
            assert_eq!(event, TerminalEvent::Started(WorkerStarted { number: 5 }));
        }
        _ => panic!("expected subscription event frame"),
    }
}

#[test]
fn macro_generated_reply_works_with_positioned_subreply() {
    let reply = signal_frame::Reply::committed(NonEmpty::single(SubReply::Ok(
        MessageReply::Accepted(Receipt { accepted: true }),
    )));

    match reply {
        signal_frame::Reply::Accepted { per_operation, .. } => {
            assert_eq!(per_operation.len(), 1);
        }
        signal_frame::Reply::Rejected { .. } => panic!("expected accepted reply"),
    }
}

// Observable channel: the macro injects observer-subscription
// operations, a stream, the publish surface, and a filter-match trait
// the contract author implements against its own filter type.

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub enum LedgerObserverFilter {
    All,
    OnlyOperations,
    OnlyEffects,
}

impl NotaEncode for LedgerObserverFilter {
    fn encode(&self, encoder: &mut nota_codec::Encoder) -> nota_codec::Result<()> {
        match self {
            LedgerObserverFilter::All => {
                encoder.start_record("All")?;
                encoder.end_record()
            }
            LedgerObserverFilter::OnlyOperations => {
                encoder.start_record("OnlyOperations")?;
                encoder.end_record()
            }
            LedgerObserverFilter::OnlyEffects => {
                encoder.start_record("OnlyEffects")?;
                encoder.end_record()
            }
        }
    }
}

impl NotaDecode for LedgerObserverFilter {
    fn decode(decoder: &mut nota_codec::Decoder<'_>) -> nota_codec::Result<Self> {
        let head = decoder.peek_record_head()?;
        match head.as_str() {
            "All" => {
                decoder.expect_record_head("All")?;
                decoder.expect_record_end()?;
                Ok(Self::All)
            }
            "OnlyOperations" => {
                decoder.expect_record_head("OnlyOperations")?;
                decoder.expect_record_end()?;
                Ok(Self::OnlyOperations)
            }
            "OnlyEffects" => {
                decoder.expect_record_head("OnlyEffects")?;
                decoder.expect_record_end()?;
                Ok(Self::OnlyEffects)
            }
            other => Err(nota_codec::Error::UnknownKindForVerb {
                verb: "LedgerObserverFilter",
                got: other.to_string(),
            }),
        }
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct LedgerNote {
    body: String,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct LedgerAcknowledgement {
    accepted: bool,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct OperationReceived {
    operation_kind: String,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct EffectEmitted {
    effect_label: String,
}

signal_channel! {
    channel Ledger {
        operation Record(LedgerNote),
    }
    reply LedgerReply {
        Recorded(LedgerAcknowledgement),
    }
    observable {
        filter LedgerObserverFilter;
        operation_event OperationReceived;
        effect_event EffectEmitted;
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    name: String,
}

signal_channel! {
    channel DomainObserve {
        operation Observe(Selection),
    }
    reply DomainObserveReply {
        Observed(LedgerAcknowledgement),
    }
    observable {
        filter LedgerObserverFilter;
        operation_event OperationReceived;
        effect_event EffectEmitted;
    }
}

impl DomainObserveObserverFilterMatch for LedgerObserverFilter {
    fn matches_operation_received(&self, _event: &OperationReceived) -> bool {
        matches!(self, Self::All | Self::OnlyOperations)
    }

    fn matches_effect_emitted(&self, _event: &EffectEmitted) -> bool {
        matches!(self, Self::All | Self::OnlyEffects)
    }
}

impl LedgerObserverFilterMatch for LedgerObserverFilter {
    fn matches_operation_received(&self, _event: &OperationReceived) -> bool {
        matches!(self, Self::All | Self::OnlyOperations)
    }

    fn matches_effect_emitted(&self, _event: &EffectEmitted) -> bool {
        matches!(self, Self::All | Self::OnlyEffects)
    }
}

#[test]
fn observable_allows_domain_observe_operation_alongside_macro_mandated_tap() {
    // The macro mandates Tap/Untap for the observable surface, so a
    // domain `Observe(...)` operation never collides with the
    // observability open verb (per /246 §2).
    let observe = DomainObserveOperation::Observe(Selection {
        name: "recent".to_string(),
    });
    assert_eq!(observe.kind(), DomainObserveOperationKind::Observe);
    assert_eq!(observe.opened_stream(), None);

    let tap = DomainObserveOperation::Tap(LedgerObserverFilter::All);
    assert_eq!(tap.kind(), DomainObserveOperationKind::Tap);
    assert_eq!(
        tap.opened_stream(),
        Some(DomainObserveStreamKind::ObserverStream)
    );
}

#[test]
fn observable_block_injects_macro_mandated_tap_and_untap_operations() {
    // Per /246 §2: when `observable { ... }` is declared the macro
    // injects the fixed `Tap(<Filter>)` / `Untap(<Token>)` operations
    // so `persona-introspect` sees a uniform vocabulary across every
    // observable channel.
    let tap = LedgerOperation::Tap(LedgerObserverFilter::All);
    assert_eq!(tap.kind(), LedgerOperationKind::Tap);
    assert_eq!(tap.opened_stream(), Some(LedgerStreamKind::ObserverStream));

    let token = LedgerObserverSubscriptionToken::new(signal_frame::SubscriptionTokenInner::new(42));
    let untap = LedgerOperation::Untap(token);
    assert_eq!(untap.kind(), LedgerOperationKind::Untap);
    assert_eq!(
        untap.closed_stream(),
        Some(LedgerStreamKind::ObserverStream)
    );
}

#[test]
fn observable_block_injects_observer_stream_and_event_classes() {
    let received = LedgerEvent::OperationReceived(OperationReceived {
        operation_kind: "Record".to_string(),
    });
    assert_eq!(received.stream_kind(), LedgerStreamKind::ObserverStream);

    let emitted = LedgerEvent::EffectEmitted(EffectEmitted {
        effect_label: "Assert".to_string(),
    });
    assert_eq!(emitted.stream_kind(), LedgerStreamKind::ObserverStream);
}

#[test]
fn observable_block_injects_reply_variant_with_freshly_minted_token() {
    let token = LedgerObserverSubscriptionToken::new(signal_frame::SubscriptionTokenInner::new(7));
    let opened = LedgerObserverSubscriptionOpened::new(token);
    let reply = LedgerReply::ObserverSubscriptionOpened(opened);
    assert_eq!(reply.kind(), LedgerReplyKind::ObserverSubscriptionOpened);
}

#[test]
fn observable_round_trips_tap_and_untap_through_nota() {
    let tap_request = LedgerOperation::Tap(LedgerObserverFilter::OnlyOperations).into_request();
    let tap_text = encode_to_text(&tap_request);
    assert_eq!(tap_text, "(Tap (OnlyOperations))");

    let mut decoder = nota_codec::Decoder::new(&tap_text);
    let decoded = Request::<LedgerOperation>::decode(&mut decoder).expect("decode tap");
    assert_eq!(decoded, tap_request);

    let token = LedgerObserverSubscriptionToken::new(signal_frame::SubscriptionTokenInner::new(9));
    let untap_request = LedgerOperation::Untap(token).into_request();
    let untap_text = encode_to_text(&untap_request);
    assert_eq!(untap_text, "(Untap (ObserverSubscriptionToken 9))");

    let mut decoder = nota_codec::Decoder::new(&untap_text);
    let decoded_untap = Request::<LedgerOperation>::decode(&mut decoder).expect("decode untap");
    assert_eq!(decoded_untap, untap_request);
}

#[test]
fn observable_round_trips_observer_subscription_opened_reply() {
    let token = LedgerObserverSubscriptionToken::new(signal_frame::SubscriptionTokenInner::new(3));
    let opened = LedgerObserverSubscriptionOpened::new(token);
    let reply_payload = LedgerReply::ObserverSubscriptionOpened(opened);

    let text = encode_to_text(&reply_payload);
    assert_eq!(
        text,
        "(ObserverSubscriptionOpened (ObserverSubscriptionOpened (ObserverSubscriptionToken 3)))"
    );

    let mut decoder = nota_codec::Decoder::new(&text);
    let decoded = LedgerReply::decode(&mut decoder).expect("decode opened");
    assert_eq!(decoded, reply_payload);
}

#[test]
fn observable_round_trips_operation_received_event() {
    let event = LedgerEvent::OperationReceived(OperationReceived {
        operation_kind: "Record".to_string(),
    });

    let text = encode_to_text(&event);
    // Outer variant records the macro-injected `OperationReceived`
    // wire head; the inner record head comes from the contract
    // author's NotaRecord-derived struct of the same name.
    let mut decoder = nota_codec::Decoder::new(&text);
    let decoded = LedgerEvent::decode(&mut decoder).expect("decode event");
    assert_eq!(decoded, event);
}

#[test]
fn observable_observer_set_routes_events_to_matching_observers() {
    let mut observer_set = LedgerObserverSet::new();

    let all_token = observer_set.register(LedgerObserverFilter::All);
    let ops_only_token = observer_set.register(LedgerObserverFilter::OnlyOperations);
    let effects_only_token = observer_set.register(LedgerObserverFilter::OnlyEffects);

    assert_ne!(all_token, ops_only_token);
    assert_ne!(ops_only_token, effects_only_token);
    assert_eq!(observer_set.len(), 3);
    assert!(!observer_set.is_empty());

    let op_event = OperationReceived {
        operation_kind: "Record".to_string(),
    };
    let mut op_recipients: Vec<LedgerObserverSubscriptionToken> = Vec::new();
    observer_set.publish_operation_received(&op_event, |token, _event| {
        op_recipients.push(token);
    });
    assert_eq!(op_recipients, vec![all_token, ops_only_token]);

    let effect_event = EffectEmitted {
        effect_label: "Assert".to_string(),
    };
    let mut effect_recipients: Vec<LedgerObserverSubscriptionToken> = Vec::new();
    observer_set.publish_effect_emitted(&effect_event, |token, _event| {
        effect_recipients.push(token);
    });
    assert_eq!(effect_recipients, vec![all_token, effects_only_token]);

    assert!(observer_set.unregister(ops_only_token));
    assert!(!observer_set.unregister(ops_only_token));
    assert_eq!(observer_set.len(), 2);

    let mut after_unregister: Vec<LedgerObserverSubscriptionToken> = Vec::new();
    observer_set.publish_operation_received(&op_event, |token, _event| {
        after_unregister.push(token);
    });
    assert_eq!(after_unregister, vec![all_token]);
}

// /246 §4: `filter default;` makes the macro emit a closed-enum
// `<Channel>ObserverFilter` with `All` / `OperationsOnly` /
// `EffectsOnly` variants and the matching `FilterMatch` impl.
// Contracts whose subscribers need richer predicates use
// `filter <CustomType>;` and write the impl themselves.

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct AuditOperationReceived {
    label: String,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct AuditEffectEmitted {
    label: String,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct AuditProbe {
    target: String,
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct AuditProbed {
    accepted: bool,
}

signal_channel! {
    channel Audit {
        operation Probe(AuditProbe),
    }
    reply AuditReply {
        Probed(AuditProbed),
    }
    observable {
        filter default;
        operation_event AuditOperationReceived;
        effect_event AuditEffectEmitted;
    }
}

#[test]
fn observable_filter_default_generates_closed_enum_filter_with_three_variants() {
    // The macro generated `AuditObserverFilter` with `All`,
    // `OperationsOnly`, `EffectsOnly` — no custom impl was needed.
    let all = AuditObserverFilter::All;
    let ops_only = AuditObserverFilter::OperationsOnly;
    let effects_only = AuditObserverFilter::EffectsOnly;

    let op_event = AuditOperationReceived {
        label: "probe".into(),
    };
    let effect_event = AuditEffectEmitted {
        label: "Assert".into(),
    };

    assert!(all.matches_operation_received(&op_event));
    assert!(all.matches_effect_emitted(&effect_event));

    assert!(ops_only.matches_operation_received(&op_event));
    assert!(!ops_only.matches_effect_emitted(&effect_event));

    assert!(!effects_only.matches_operation_received(&op_event));
    assert!(effects_only.matches_effect_emitted(&effect_event));
}

#[test]
fn observable_filter_default_wire_round_trips_through_tap_request() {
    // The macro-generated `AuditObserverFilter` rides the wire on the
    // injected `Tap(AuditObserverFilter)` operation. Round-trip its
    // three variants through the request encoder.
    for filter in [
        AuditObserverFilter::All,
        AuditObserverFilter::OperationsOnly,
        AuditObserverFilter::EffectsOnly,
    ] {
        let tap = AuditOperation::Tap(filter).into_request();
        let text = encode_to_text(&tap);

        let mut decoder = nota_codec::Decoder::new(&text);
        let decoded = Request::<AuditOperation>::decode(&mut decoder)
            .expect("default filter round-trips through Tap request");
        assert_eq!(decoded, tap);
    }
}

#[test]
fn observable_filter_default_routes_through_observer_set() {
    // The macro-generated observer set uses the macro-generated
    // FilterMatch impl when fanning events out.
    let mut observer_set = AuditObserverSet::new();
    let all_token = observer_set.register(AuditObserverFilter::All);
    let ops_only_token = observer_set.register(AuditObserverFilter::OperationsOnly);
    let effects_only_token = observer_set.register(AuditObserverFilter::EffectsOnly);

    let op_event = AuditOperationReceived {
        label: "probe".into(),
    };
    let mut op_recipients: Vec<AuditObserverSubscriptionToken> = Vec::new();
    observer_set.publish_operation_received(&op_event, |token, _event| {
        op_recipients.push(token);
    });
    assert_eq!(op_recipients, vec![all_token, ops_only_token]);

    let effect_event = AuditEffectEmitted {
        label: "Assert".into(),
    };
    let mut effect_recipients: Vec<AuditObserverSubscriptionToken> = Vec::new();
    observer_set.publish_effect_emitted(&effect_event, |token, _event| {
        effect_recipients.push(token);
    });
    assert_eq!(effect_recipients, vec![all_token, effects_only_token]);
}

#[test]
fn observable_streaming_frame_alias_carries_observer_events() {
    let event_identifier = signal_frame::StreamEventIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Acceptor,
        LaneSequence::first(),
    );
    let event = LedgerEvent::EffectEmitted(EffectEmitted {
        effect_label: "Assert".to_string(),
    });
    let frame = LedgerFrame::new(StreamingFrameBody::SubscriptionEvent {
        event_identifier,
        token: signal_frame::SubscriptionTokenInner::new(11),
        event: event.clone(),
    });

    let bytes = frame.encode_length_prefixed().expect("encode");
    let decoded =
        StreamingFrame::<LedgerOperation, LedgerReply, LedgerEvent>::decode_length_prefixed(&bytes)
            .expect("decode");

    match decoded.into_body() {
        StreamingFrameBody::SubscriptionEvent {
            event_identifier: decoded_identifier,
            token,
            event: decoded_event,
        } => {
            assert_eq!(decoded_identifier, event_identifier);
            assert_eq!(token, signal_frame::SubscriptionTokenInner::new(11));
            assert_eq!(decoded_event, event);
        }
        _ => panic!("expected subscription event frame"),
    }
}
