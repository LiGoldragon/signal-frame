#[cfg(feature = "dotos-text")]
use dotos::{Block, Delimiter, DotosBlock, DotosDecode, DotosDecodeError, DotosEncode, DotosSource};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_frame::{
    AcceptedOutcome, BatchErrorClassification, BatchFailureReason, BoundExchangeFrame,
    BoundStreamingFrame, Caller, CallerIdentity, CommitStatus, ContractBinding, ContractId,
    ExchangeFrameBody, ExchangeIdentifier, ExchangeLane, FrameError, HandshakeRejectionReason,
    HandshakeReply, HandshakeRequest, LaneSequence, NonEmpty, OperationFailureReason,
    ProcessIdentifier, ProtocolVersion, Reply, Request, RequestPayload, RetryClassification,
    Revision, RootCode, SessionEpoch, Slot, StreamEventIdentifier, StreamingFrameBody, SubReply,
    SubscriptionTokenInner, VariantCode, WireContract, WireRevision, WireRoute,
    short_header_from_archive, short_header_from_length_prefixed,
};
use std::num::{NonZeroU16, NonZeroU32};

#[derive(Debug)]
struct TestContract;
impl WireContract for TestContract {
    const BINDING: ContractBinding = ContractBinding::new(
        ContractId::new(NonZeroU32::MIN),
        WireRevision::new(NonZeroU16::MIN),
    );
}

const TEST_ROUTE: WireRoute = WireRoute::new(RootCode::new(7), VariantCode::new(8));

type TestExchangeFrame<RequestPayload, ReplyPayload> =
    BoundExchangeFrame<TestContract, RequestPayload, ReplyPayload>;
type TestStreamingFrame<RequestPayload, ReplyPayload, EventPayload> =
    BoundStreamingFrame<TestContract, RequestPayload, ReplyPayload, EventPayload>;

fn exchange_frame<RequestPayload, ReplyPayload>(
    body: ExchangeFrameBody<RequestPayload, ReplyPayload>,
) -> TestExchangeFrame<RequestPayload, ReplyPayload> {
    TestExchangeFrame::new(TEST_ROUTE, body)
}

fn streaming_frame<RequestPayload, ReplyPayload, EventPayload>(
    body: StreamingFrameBody<RequestPayload, ReplyPayload, EventPayload>,
) -> TestStreamingFrame<RequestPayload, ReplyPayload, EventPayload> {
    TestStreamingFrame::new(TEST_ROUTE, body)
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EngineFailure {
    Unavailable,
}

impl BatchErrorClassification for EngineFailure {
    fn batch_failure_reason(&self) -> BatchFailureReason {
        match self {
            Self::Unavailable => BatchFailureReason::EngineUnavailable,
        }
    }

    fn retry_classification(&self) -> RetryClassification {
        match self {
            Self::Unavailable => RetryClassification::Retryable,
        }
    }

    fn commit_status(&self) -> CommitStatus {
        match self {
            Self::Unavailable => CommitStatus::Unknown,
        }
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
    let frame = exchange_frame::<DomainRequest, DomainReply>(ExchangeFrameBody::Request {
        exchange,
        request: request.clone(),
    });

    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        TestExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes).unwrap();

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
fn request_frame_round_trips_caller_identity() {
    let request = DomainRequest::new("Node").into_request().with_caller(Some(
        Caller::new(ProcessIdentifier::new(7), None, None)
            .with_identity(Some(CallerIdentity::new("designer"))),
    ));
    let frame = exchange_frame::<DomainRequest, DomainReply>(ExchangeFrameBody::Request {
        exchange: fresh_exchange(),
        request,
    });

    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        TestExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes).unwrap();

    let ExchangeFrameBody::Request { request, .. } = decoded.into_body() else {
        panic!("expected request frame");
    };
    let identity = request
        .caller()
        .and_then(Caller::identity)
        .expect("caller identity survives frame round trip");
    assert_eq!(identity.as_str(), "designer");
}

#[test]
fn exchange_frame_always_has_the_contract_binding() {
    let frame = exchange_frame::<DomainRequest, DomainReply>(ExchangeFrameBody::HandshakeRequest(
        HandshakeRequest::current(),
    ));

    assert_eq!(frame.short_header().binding(), TestContract::BINDING);
}

#[test]
fn exchange_frame_short_header_round_trips_and_is_peekable() {
    let exchange = fresh_exchange();
    let request = DomainRequest::new("Node").into_request();
    let frame = exchange_frame::<DomainRequest, DomainReply>(ExchangeFrameBody::Request {
        exchange,
        request: request.clone(),
    });
    let short_header = frame.short_header();

    let archive = frame.encode().unwrap();
    assert_eq!(
        short_header_from_archive(&archive)
            .unwrap()
            .validate()
            .unwrap(),
        short_header
    );
    assert_eq!(&archive[..8], &short_header.to_le_bytes());

    let bytes = frame.encode_length_prefixed().unwrap();
    assert_eq!(
        short_header_from_length_prefixed(&bytes)
            .unwrap()
            .validate()
            .unwrap(),
        short_header
    );
    assert_eq!(&bytes[4..12], &short_header.to_le_bytes());

    let decoded =
        TestExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes).unwrap();
    assert_eq!(decoded.short_header(), short_header);
    assert_eq!(decoded.body(), frame.body());
}

#[test]
fn short_header_peek_rejects_short_payload() {
    let bytes = [0, 0, 0, 7, 1, 2, 3, 4, 5, 6, 7];

    let err = short_header_from_length_prefixed(&bytes).expect_err("short header must fail");
    assert!(matches!(err, FrameError::ShortHeaderTooShort { found: 7 }));
}

#[test]
fn request_from_payload_wraps_single_payload() {
    let request = Request::from_payload(DomainRequest::new("Node"));

    assert_eq!(request.payloads().len(), 1);
    assert_eq!(request.payloads().head(), &DomainRequest::new("Node"));
}

#[test]
fn handshake_request_frame_round_trips_at_frame_layer() {
    let frame = exchange_frame::<DomainRequest, DomainReply>(ExchangeFrameBody::HandshakeRequest(
        HandshakeRequest::current(),
    ));

    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        TestExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes).unwrap();

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
    let frame = exchange_frame::<DomainRequest, DomainReply>(ExchangeFrameBody::HandshakeReply(
        HandshakeReply::Rejected(HandshakeRejectionReason::IncompatibleVersion { local, peer }),
    ));

    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        TestExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes).unwrap();

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
    let frame = exchange_frame::<DomainRequest, DomainReply>(ExchangeFrameBody::Reply {
        exchange,
        reply: reply.clone(),
    });

    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        TestExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes).unwrap();

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
fn batch_error_classification_projects_wire_safe_metadata() {
    let failure = EngineFailure::Unavailable;

    assert_eq!(
        failure.batch_failure_reason(),
        BatchFailureReason::EngineUnavailable
    );
    assert_eq!(
        failure.retry_classification(),
        RetryClassification::Retryable
    );
    assert_eq!(failure.commit_status(), CommitStatus::Unknown);
}

#[test]
fn typed_slot_and_revision_round_trip_inside_request_frame() {
    let payload = SlotRequest::new(Slot::new(42), Revision::initial());
    let exchange = fresh_exchange();
    let frame = exchange_frame::<SlotRequest, DomainReply>(ExchangeFrameBody::Request {
        exchange,
        request: payload.clone().into_request(),
    });

    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        TestExchangeFrame::<SlotRequest, DomainReply>::decode_length_prefixed(&bytes).unwrap();

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
    let err = TestExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&[0, 0, 0])
        .expect_err("short prefix must fail");

    assert!(matches!(err, FrameError::ShortLengthPrefix));
}

#[test]
fn length_prefixed_decode_rejects_short_payload() {
    let bytes = [0, 0, 0, 4, 1, 2, 3];
    let err = TestExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes)
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
    let err = TestExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes)
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
    let event_identifier =
        StreamEventIdentifier::acceptor(SessionEpoch::new(1), LaneSequence::first());
    let frame: TestStreamingFrame<DomainRequest, DomainReply, DomainEvent> =
        streaming_frame(StreamingFrameBody::SubscriptionEvent {
            event_identifier,
            token: SubscriptionTokenInner::new(7),
            event: DomainEvent {
                note: "thought arrived".into(),
            },
        });

    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        TestStreamingFrame::<DomainRequest, DomainReply, DomainEvent>::decode_length_prefixed(
            &bytes,
        )
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

#[test]
fn streaming_frame_short_header_round_trips_and_is_peekable() {
    let event_identifier =
        StreamEventIdentifier::acceptor(SessionEpoch::new(1), LaneSequence::first());
    let frame: TestStreamingFrame<DomainRequest, DomainReply, DomainEvent> =
        streaming_frame(StreamingFrameBody::SubscriptionEvent {
            event_identifier,
            token: SubscriptionTokenInner::new(7),
            event: DomainEvent {
                note: "thought arrived".into(),
            },
        });
    let short_header = frame.short_header();

    let bytes = frame.encode_length_prefixed().unwrap();
    assert_eq!(
        short_header_from_length_prefixed(&bytes)
            .unwrap()
            .validate()
            .unwrap(),
        short_header
    );
    assert_eq!(&bytes[4..12], &short_header.to_le_bytes());

    let decoded =
        TestStreamingFrame::<DomainRequest, DomainReply, DomainEvent>::decode_length_prefixed(
            &bytes,
        )
        .unwrap();
    assert_eq!(decoded.short_header(), short_header);
    assert_eq!(decoded.body(), frame.body());
}

// ─── Request DOTOS witnesses under the contract-local shape ───
//
// Under the contract-local-verb architecture, `Request<Payload>`
// holds payloads directly — no outer verb wrapper, no per-operation
// wrapper. The payload (typically an enum produced by the
// `signal_channel!` macro) emits its own record head naming the
// contract-local verb.

#[cfg(feature = "dotos-text")]
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct DotosSubmit {
    text: String,
}

#[cfg(feature = "dotos-text")]
impl RequestPayload for DotosSubmit {}

#[cfg(feature = "dotos-text")]
impl DotosEncode for DotosSubmit {
    fn to_dotos(&self) -> String {
        Delimiter::Parenthesis.wrap(["Submit".to_owned(), self.text.to_dotos()])
    }
}

#[cfg(feature = "dotos-text")]
impl DotosDecode for DotosSubmit {
    fn from_dotos_block(block: &Block) -> Result<Self, DotosDecodeError> {
        let children =
            DotosBlock::new(block).expect_children(Delimiter::Parenthesis, "Submit", 2)?;
        let head = children[0]
            .demote_to_string()
            .ok_or(DotosDecodeError::ExpectedAtom {
                type_name: "Submit",
            })?;
        if head != "Submit" {
            return Err(DotosDecodeError::UnknownVariant {
                enum_name: "DotosSubmit",
                variant: head.to_owned(),
            });
        }
        let text = String::from_dotos_block(&children[1])?;
        Ok(Self { text })
    }
}

#[cfg(feature = "dotos-text")]
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct DotosInbox {
    name: String,
}

#[cfg(feature = "dotos-text")]
impl RequestPayload for DotosInbox {}

#[cfg(feature = "dotos-text")]
impl DotosEncode for DotosInbox {
    fn to_dotos(&self) -> String {
        Delimiter::Parenthesis.wrap(["Inbox".to_owned(), self.name.to_dotos()])
    }
}

#[cfg(feature = "dotos-text")]
impl DotosDecode for DotosInbox {
    fn from_dotos_block(block: &Block) -> Result<Self, DotosDecodeError> {
        let children = DotosBlock::new(block).expect_children(Delimiter::Parenthesis, "Inbox", 2)?;
        let head = children[0]
            .demote_to_string()
            .ok_or(DotosDecodeError::ExpectedAtom { type_name: "Inbox" })?;
        if head != "Inbox" {
            return Err(DotosDecodeError::UnknownVariant {
                enum_name: "DotosInbox",
                variant: head.to_owned(),
            });
        }
        let name = String::from_dotos_block(&children[1])?;
        Ok(Self { name })
    }
}

#[cfg(feature = "dotos-text")]
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
enum DotosChannelRequest {
    Submit(DotosSubmit),
    Inbox(DotosInbox),
}

#[cfg(feature = "dotos-text")]
impl RequestPayload for DotosChannelRequest {}

#[cfg(feature = "dotos-text")]
impl DotosEncode for DotosChannelRequest {
    fn to_dotos(&self) -> String {
        match self {
            Self::Submit(payload) => payload.to_dotos(),
            Self::Inbox(payload) => payload.to_dotos(),
        }
    }
}

#[cfg(feature = "dotos-text")]
impl DotosDecode for DotosChannelRequest {
    fn from_dotos_block(block: &Block) -> Result<Self, DotosDecodeError> {
        let children = DotosBlock::new(block).expect_children(
            Delimiter::Parenthesis,
            "DotosChannelRequest",
            2,
        )?;
        let head = children[0]
            .demote_to_string()
            .ok_or(DotosDecodeError::ExpectedAtom {
                type_name: "DotosChannelRequest",
            })?;
        match head {
            "Submit" => Ok(Self::Submit(DotosSubmit::from_dotos_block(block)?)),
            "Inbox" => Ok(Self::Inbox(DotosInbox::from_dotos_block(block)?)),
            other => Err(DotosDecodeError::UnknownVariant {
                enum_name: "DotosChannelRequest",
                variant: other.to_string(),
            }),
        }
    }
}

#[cfg(feature = "dotos-text")]
fn encode_to_text<T: DotosEncode>(value: &T) -> String {
    value.to_dotos()
}

#[cfg(feature = "dotos-text")]
fn decode_request_from_text(text: &str) -> Result<Request<DotosChannelRequest>, DotosDecodeError> {
    DotosSource::new(text).parse::<Request<DotosChannelRequest>>()
}

#[test]
#[cfg(feature = "dotos-text")]
fn single_op_request_round_trips_without_outer_verb_wrapper() {
    let payload = DotosChannelRequest::Submit(DotosSubmit {
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
#[cfg(feature = "dotos-text")]
fn multi_op_request_round_trips_through_sequence() {
    let request = Request::from_payloads(NonEmpty::from_head_and_tail(
        DotosChannelRequest::Submit(DotosSubmit { text: "one".into() }),
        vec![DotosChannelRequest::Inbox(DotosInbox {
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
    let frame: TestStreamingFrame<DomainRequest, DomainReply, DomainEvent> =
        streaming_frame(StreamingFrameBody::Request {
            exchange,
            request: request.clone(),
        });
    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        TestStreamingFrame::<DomainRequest, DomainReply, DomainEvent>::decode_length_prefixed(
            &bytes,
        )
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
