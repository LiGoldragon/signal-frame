use nota_codec::{NotaDecode, NotaEncode, NotaRecord};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_frame::{
    ExchangeFrame, ExchangeFrameBody, ExchangeIdentifier, ExchangeLane, LaneSequence, NonEmpty,
    Request, RequestPayload, SessionEpoch, StreamingFrame, StreamingFrameBody, SubReply,
    signal_channel,
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
            assert_eq!(
                event,
                TerminalEvent::Started(WorkerStarted { number: 5 })
            );
        }
        _ => panic!("expected subscription event frame"),
    }
}

#[test]
fn macro_generated_reply_works_with_positioned_subreply() {
    let reply = signal_frame::Reply::completed(NonEmpty::single(SubReply::Ok {
        payload: MessageReply::Accepted(Receipt { accepted: true }),
    }));

    match reply {
        signal_frame::Reply::Accepted { per_operation, .. } => {
            assert_eq!(per_operation.len(), 1);
        }
        signal_frame::Reply::Rejected { .. } => panic!("expected accepted reply"),
    }
}
