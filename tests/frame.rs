use nota_codec::{NotaDecode, NotaEncode};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_frame::{
    AcceptedOutcome, BatchFailureReason, CommitStatus, ExchangeFrame, ExchangeFrameBody,
    ExchangeIdentifier, ExchangeLane, FrameError, HandshakeRejectionReason, HandshakeReply,
    HandshakeRequest, LaneSequence, NonEmpty, OperationFailureReason, ProtocolVersion, Reply,
    Request, RequestPayload, RetryClassification, Revision, SessionEpoch, Slot,
    StreamEventIdentifier, StreamingFrame, StreamingFrameBody, SubReply, SubscriptionTokenInner,
};

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct DomainRequest {
    name: String,
}

impl DomainRequest {
    fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl RequestPayload for DomainRequest {}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct DomainReply {
    accepted: bool,
}

impl DomainReply {
    fn accepted() -> Self {
        Self { accepted: true }
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct Node;

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct SlotRequest {
    slot: Slot<Node>,
    expected_revision: Revision,
}

impl SlotRequest {
    fn new(slot: Slot<Node>, expected_revision: Revision) -> Self {
        Self {
            slot,
            expected_revision,
        }
    }
}

impl RequestPayload for SlotRequest {}

fn fresh_exchange() -> ExchangeIdentifier {
    ExchangeIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Connector,
        LaneSequence::first(),
    )
}

#[test]
fn request_frame_round_trips() {
    let exchange = fresh_exchange();
    let request = DomainRequest::new("Node").into_request();
    let frame = ExchangeFrame::<DomainRequest, DomainReply>::new(ExchangeFrameBody::Request {
        exchange,
        request: request.clone(),
    });

    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        ExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes).unwrap();

    match decoded.into_body() {
        ExchangeFrameBody::Request {
            exchange: decoded_exchange,
            request: decoded_request,
        } => {
            assert_eq!(decoded_exchange, exchange);
            assert_eq!(decoded_request, request);
            assert_eq!(
                decoded_request.payloads().head(),
                &DomainRequest::new("Node")
            );
        }
        _ => panic!("expected request frame"),
    }
}

#[test]
fn request_from_payload_wraps_single_payload() {
    let request = Request::from_payload(DomainRequest::new("Node"));

    assert_eq!(request.payloads().len(), 1);
    assert_eq!(request.payloads().head(), &DomainRequest::new("Node"));
}

#[test]
fn handshake_request_frame_round_trips_at_frame_layer() {
    let frame = ExchangeFrame::<DomainRequest, DomainReply>::new(
        ExchangeFrameBody::HandshakeRequest(HandshakeRequest::current()),
    );

    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        ExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes).unwrap();

    match decoded.into_body() {
        ExchangeFrameBody::HandshakeRequest(request) => {
            assert_eq!(request.version(), HandshakeRequest::current().version());
        }
        _ => panic!("expected handshake request frame"),
    }
}

#[test]
fn handshake_rejection_frame_round_trips_at_frame_layer() {
    let local = ProtocolVersion::new(0, 1, 0);
    let peer = ProtocolVersion::new(1, 0, 0);
    let frame =
        ExchangeFrame::<DomainRequest, DomainReply>::new(ExchangeFrameBody::HandshakeReply(
            HandshakeReply::Rejected(HandshakeRejectionReason::IncompatibleVersion { local, peer }),
        ));

    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        ExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes).unwrap();

    match decoded.into_body() {
        ExchangeFrameBody::HandshakeReply(HandshakeReply::Rejected(
            HandshakeRejectionReason::IncompatibleVersion { local, peer },
        )) => {
            assert_eq!(local, ProtocolVersion::new(0, 1, 0));
            assert_eq!(peer, ProtocolVersion::new(1, 0, 0));
        }
        _ => panic!("expected handshake rejection frame"),
    }
}

#[test]
fn reply_frame_round_trips_with_exchange_identifier() {
    let exchange = fresh_exchange();
    let reply = Reply::committed(NonEmpty::single(SubReply::Ok(DomainReply::accepted())));
    let frame = ExchangeFrame::<DomainRequest, DomainReply>::new(ExchangeFrameBody::Reply {
        exchange,
        reply: reply.clone(),
    });

    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        ExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes).unwrap();

    match decoded.into_body() {
        ExchangeFrameBody::Reply {
            exchange: decoded_exchange,
            reply: decoded_reply,
        } => {
            assert_eq!(decoded_exchange, exchange);
            assert_eq!(decoded_reply, reply);
        }
        _ => panic!("expected reply frame"),
    }
}

#[test]
fn operation_aborted_reply_carries_failed_at_and_per_operation_subreplies() {
    let per_operation = NonEmpty::from_head_and_tail(
        SubReply::<DomainReply>::Invalidated,
        vec![
            SubReply::<DomainReply>::Failed {
                reason: OperationFailureReason::DomainRejection,
                detail: Some(DomainReply::accepted()),
            },
            SubReply::Skipped,
        ],
    );
    let reply = Reply::<DomainReply>::operation_aborted(
        1,
        OperationFailureReason::DomainRejection,
        per_operation,
    );

    match &reply {
        Reply::Accepted {
            outcome,
            per_operation,
        } => {
            assert!(matches!(
                outcome,
                AcceptedOutcome::OperationAborted {
                    failed_at: 1,
                    reason: OperationFailureReason::DomainRejection,
                }
            ));
            assert_eq!(per_operation.len(), 3);
        }
        Reply::Rejected { .. } => panic!("expected accepted reply"),
    }
}

#[test]
fn batch_aborted_reply_carries_batch_reason_and_per_operation_subreplies() {
    let per_operation = NonEmpty::from_head_and_tail(
        SubReply::<DomainReply>::Invalidated,
        vec![SubReply::<DomainReply>::Invalidated],
    );
    let reply = Reply::<DomainReply>::batch_aborted(
        BatchFailureReason::EngineRejected,
        RetryClassification::Unknown,
        CommitStatus::NotCommitted,
        per_operation,
    );

    match &reply {
        Reply::Accepted {
            outcome,
            per_operation,
        } => {
            assert!(matches!(
                outcome,
                AcceptedOutcome::BatchAborted {
                    reason: BatchFailureReason::EngineRejected,
                    retry: RetryClassification::Unknown,
                    commit: CommitStatus::NotCommitted,
                }
            ));
            assert_eq!(per_operation.len(), 2);
            assert!(
                per_operation
                    .iter()
                    .all(|reply| matches!(reply, SubReply::Invalidated)),
            );
        }
        Reply::Rejected { .. } => panic!("expected accepted reply"),
    }
}

#[test]
fn typed_slot_and_revision_round_trip_inside_request_frame() {
    let payload = SlotRequest::new(Slot::new(42), Revision::initial());
    let exchange = fresh_exchange();
    let frame = ExchangeFrame::<SlotRequest, DomainReply>::new(ExchangeFrameBody::Request {
        exchange,
        request: payload.clone().into_request(),
    });

    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        ExchangeFrame::<SlotRequest, DomainReply>::decode_length_prefixed(&bytes).unwrap();

    match decoded.into_body() {
        ExchangeFrameBody::Request {
            exchange: decoded_exchange,
            request,
        } => {
            assert_eq!(decoded_exchange, exchange);
            let head = request.payloads().head();
            assert_eq!(head, &payload);
            assert_eq!(head.slot.number(), 42);
            assert_eq!(head.expected_revision.number(), 0);
        }
        _ => panic!("expected request frame"),
    }
}

#[test]
fn length_prefixed_decode_rejects_short_prefix() {
    let err = ExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&[0, 0, 0])
        .expect_err("short prefix must fail");

    assert!(matches!(err, FrameError::ShortLengthPrefix));
}

#[test]
fn length_prefixed_decode_rejects_short_payload() {
    let bytes = [0, 0, 0, 4, 1, 2, 3];
    let err = ExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes)
        .expect_err("short payload must fail");

    assert!(matches!(
        err,
        FrameError::LengthMismatch {
            expected: 4,
            found: 3
        }
    ));
}

#[test]
fn length_prefixed_decode_rejects_long_payload() {
    let bytes = [0, 0, 0, 2, 1, 2, 3];
    let err = ExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes)
        .expect_err("long payload must fail");

    assert!(matches!(
        err,
        FrameError::LengthMismatch {
            expected: 2,
            found: 3
        }
    ));
}

#[test]
fn protocol_version_accepts_same_major_and_not_newer_minor() {
    let local = ProtocolVersion::new(0, 2, 0);

    assert!(local.accepts(ProtocolVersion::new(0, 1, 9)));
    assert!(local.accepts(ProtocolVersion::new(0, 2, 0)));
    assert!(!local.accepts(ProtocolVersion::new(0, 3, 0)));
    assert!(!local.accepts(ProtocolVersion::new(1, 0, 0)));
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct DomainEvent {
    note: String,
}

#[test]
fn streaming_frame_subscription_event_round_trips() {
    let event_identifier = StreamEventIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Acceptor,
        LaneSequence::first(),
    );
    let frame: StreamingFrame<DomainRequest, DomainReply, DomainEvent> =
        StreamingFrame::new(StreamingFrameBody::SubscriptionEvent {
            event_identifier,
            token: SubscriptionTokenInner::new(7),
            event: DomainEvent {
                note: "thought arrived".into(),
            },
        });

    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        StreamingFrame::<DomainRequest, DomainReply, DomainEvent>::decode_length_prefixed(&bytes)
            .unwrap();

    match decoded.into_body() {
        StreamingFrameBody::SubscriptionEvent {
            event_identifier: decoded_id,
            token,
            event,
        } => {
            assert_eq!(decoded_id, event_identifier);
            assert_eq!(token, SubscriptionTokenInner::new(7));
            assert_eq!(event.note, "thought arrived");
        }
        _ => panic!("expected subscription event"),
    }
}

// ─── Request NOTA witnesses under the contract-local shape ───
//
// Under the contract-local-verb architecture, `Request<Payload>`
// holds payloads directly — no outer verb wrapper, no per-operation
// wrapper. The payload (typically an enum produced by the
// `signal_channel!` macro) emits its own record head naming the
// contract-local verb.

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct NotaSubmit {
    text: String,
}

impl RequestPayload for NotaSubmit {}

impl NotaEncode for NotaSubmit {
    fn encode(&self, encoder: &mut nota_codec::Encoder) -> nota_codec::Result<()> {
        encoder.start_record("Submit")?;
        self.text.encode(encoder)?;
        encoder.end_record()
    }
}

impl NotaDecode for NotaSubmit {
    fn decode(decoder: &mut nota_codec::Decoder<'_>) -> nota_codec::Result<Self> {
        decoder.expect_record_head("Submit")?;
        let text = String::decode(decoder)?;
        decoder.expect_record_end()?;
        Ok(Self { text })
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct NotaInbox {
    name: String,
}

impl RequestPayload for NotaInbox {}

impl NotaEncode for NotaInbox {
    fn encode(&self, encoder: &mut nota_codec::Encoder) -> nota_codec::Result<()> {
        encoder.start_record("Inbox")?;
        self.name.encode(encoder)?;
        encoder.end_record()
    }
}

impl NotaDecode for NotaInbox {
    fn decode(decoder: &mut nota_codec::Decoder<'_>) -> nota_codec::Result<Self> {
        decoder.expect_record_head("Inbox")?;
        let name = String::decode(decoder)?;
        decoder.expect_record_end()?;
        Ok(Self { name })
    }
}

#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
enum NotaChannelRequest {
    Submit(NotaSubmit),
    Inbox(NotaInbox),
}

impl RequestPayload for NotaChannelRequest {}

impl NotaEncode for NotaChannelRequest {
    fn encode(&self, encoder: &mut nota_codec::Encoder) -> nota_codec::Result<()> {
        match self {
            Self::Submit(payload) => payload.encode(encoder),
            Self::Inbox(payload) => payload.encode(encoder),
        }
    }
}

impl NotaDecode for NotaChannelRequest {
    fn decode(decoder: &mut nota_codec::Decoder<'_>) -> nota_codec::Result<Self> {
        let head = decoder.peek_record_head()?;
        match head.as_str() {
            "Submit" => Ok(Self::Submit(NotaSubmit::decode(decoder)?)),
            "Inbox" => Ok(Self::Inbox(NotaInbox::decode(decoder)?)),
            other => Err(nota_codec::Error::UnknownKindForVerb {
                verb: "NotaChannelRequest",
                got: other.to_string(),
            }),
        }
    }
}

fn encode_to_text<T: NotaEncode>(value: &T) -> String {
    let mut encoder = nota_codec::Encoder::new();
    value.encode(&mut encoder).expect("encode");
    encoder.into_string()
}

fn decode_request_from_text(text: &str) -> nota_codec::Result<Request<NotaChannelRequest>> {
    let mut decoder = nota_codec::Decoder::new(text);
    Request::<NotaChannelRequest>::decode(&mut decoder)
}

#[test]
fn single_op_request_round_trips_without_outer_verb_wrapper() {
    let payload = NotaChannelRequest::Submit(NotaSubmit {
        text: "hello".into(),
    });
    let request = payload.into_request();
    let text = encode_to_text(&request);

    // No `(Assert ...)` outer wrapper — the payload itself names the
    // contract-local verb via its record head.
    assert_eq!(text, "(Submit hello)");

    let decoded = decode_request_from_text(&text).expect("decode");
    assert_eq!(decoded, request);
    assert_eq!(decoded.payloads.len(), 1);
}

#[test]
fn multi_op_request_round_trips_through_sequence() {
    let request = Request::from_payloads(NonEmpty::from_head_and_tail(
        NotaChannelRequest::Submit(NotaSubmit { text: "one".into() }),
        vec![NotaChannelRequest::Inbox(NotaInbox {
            name: "operator".into(),
        })],
    ));
    let text = encode_to_text(&request);

    assert_eq!(text, "[(Submit one) (Inbox operator)]");

    let decoded = decode_request_from_text(&text).expect("decode");
    assert_eq!(decoded, request);
    assert_eq!(decoded.payloads.len(), 2);
}

#[test]
fn streaming_frame_request_round_trips() {
    let exchange = fresh_exchange();
    let request = DomainRequest::new("Node").into_request();
    let frame: StreamingFrame<DomainRequest, DomainReply, DomainEvent> =
        StreamingFrame::new(StreamingFrameBody::Request {
            exchange,
            request: request.clone(),
        });
    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        StreamingFrame::<DomainRequest, DomainReply, DomainEvent>::decode_length_prefixed(&bytes)
            .unwrap();
    match decoded.into_body() {
        StreamingFrameBody::Request {
            exchange: decoded_exchange,
            request: decoded_request,
        } => {
            assert_eq!(decoded_exchange, exchange);
            assert_eq!(decoded_request, request);
        }
        _ => panic!("expected request frame on streaming channel"),
    }
}
