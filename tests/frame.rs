#[cfg(feature = "nota-text")]
use nota::{Block, Delimiter, NotaBlock, NotaDecode, NotaDecodeError, NotaEncode, NotaSource};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_frame::{
    AcceptedOutcome, BatchErrorClassification, BatchFailureReason, Caller, CallerIdentity,
    CommitStatus, ExchangeFrame, ExchangeFrameBody, ExchangeIdentifier, ExchangeLane, FrameError,
    HandshakeRejectionReason, HandshakeReply, HandshakeRequest, LaneSequence, NonEmpty,
    OperationFailureReason, ProcessIdentifier, ProtocolVersion, Reply, Request, RequestPayload,
    RetryClassification, Revision, SessionEpoch, ShortHeader, Slot, StreamEventIdentifier,
    StreamingFrame, StreamingFrameBody, SubReply, SubscriptionTokenInner,
    short_header_from_archive, short_header_from_length_prefixed,
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
fn request_frame_round_trips_caller_identity() {
    let request = DomainRequest::new("Node").into_request().with_caller(Some(
        Caller::new(ProcessIdentifier::new(7), None, None)
            .with_identity(Some(CallerIdentity::new("designer"))),
    ));
    let frame = ExchangeFrame::<DomainRequest, DomainReply>::new(ExchangeFrameBody::Request {
        exchange: fresh_exchange(),
        request,
    });

    let bytes = frame.encode_length_prefixed().unwrap();
    let decoded =
        ExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes).unwrap();

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
fn exchange_frame_defaults_to_empty_short_header() {
    let frame = ExchangeFrame::<DomainRequest, DomainReply>::new(
        ExchangeFrameBody::HandshakeRequest(HandshakeRequest::current()),
    );

    assert_eq!(frame.short_header(), ShortHeader::empty());
}

#[test]
fn exchange_frame_short_header_round_trips_and_is_peekable() {
    let exchange = fresh_exchange();
    let short_header = ShortHeader::new(0x0807_0605_0403_0201);
    let request = DomainRequest::new("Node").into_request();
    let frame = ExchangeFrame::<DomainRequest, DomainReply>::with_short_header(
        short_header,
        ExchangeFrameBody::Request {
            exchange,
            request: request.clone(),
        },
    );

    let archive = frame.encode().unwrap();
    assert_eq!(short_header_from_archive(&archive).unwrap(), short_header);
    assert_eq!(&archive[..8], &short_header.to_le_bytes());

    let bytes = frame.encode_length_prefixed().unwrap();
    assert_eq!(
        short_header_from_length_prefixed(&bytes).unwrap(),
        short_header
    );
    assert_eq!(&bytes[4..12], &short_header.to_le_bytes());

    let decoded =
        ExchangeFrame::<DomainRequest, DomainReply>::decode_length_prefixed(&bytes).unwrap();
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

#[test]
fn streaming_frame_short_header_round_trips_and_is_peekable() {
    let event_identifier = StreamEventIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Acceptor,
        LaneSequence::first(),
    );
    let short_header = ShortHeader::new(0x0102_0304_0506_0708);
    let frame: StreamingFrame<DomainRequest, DomainReply, DomainEvent> =
        StreamingFrame::with_short_header(
            short_header,
            StreamingFrameBody::SubscriptionEvent {
                event_identifier,
                token: SubscriptionTokenInner::new(7),
                event: DomainEvent {
                    note: "thought arrived".into(),
                },
            },
        );

    let bytes = frame.encode_length_prefixed().unwrap();
    assert_eq!(
        short_header_from_length_prefixed(&bytes).unwrap(),
        short_header
    );
    assert_eq!(&bytes[4..12], &short_header.to_le_bytes());

    let decoded =
        StreamingFrame::<DomainRequest, DomainReply, DomainEvent>::decode_length_prefixed(&bytes)
            .unwrap();
    assert_eq!(decoded.short_header(), short_header);
    assert_eq!(decoded.body(), frame.body());
}

// ─── Request NOTA witnesses under the contract-local shape ───
//
// Under the contract-local-verb architecture, `Request<Payload>`
// holds payloads directly — no outer verb wrapper, no per-operation
// wrapper. The payload (typically an enum produced by the
// `signal_channel!` macro) emits its own record head naming the
// contract-local verb.

#[cfg(feature = "nota-text")]
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct NotaSubmit {
    text: String,
}

#[cfg(feature = "nota-text")]
impl RequestPayload for NotaSubmit {}

#[cfg(feature = "nota-text")]
impl NotaEncode for NotaSubmit {
    fn to_nota(&self) -> String {
        Delimiter::Parenthesis.wrap(["Submit".to_owned(), self.text.to_nota()])
    }
}

#[cfg(feature = "nota-text")]
impl NotaDecode for NotaSubmit {
    fn from_nota_block(block: &Block) -> Result<Self, NotaDecodeError> {
        let children =
            NotaBlock::new(block).expect_children(Delimiter::Parenthesis, "Submit", 2)?;
        let head = children[0]
            .demote_to_string()
            .ok_or(NotaDecodeError::ExpectedAtom {
                type_name: "Submit",
            })?;
        if head != "Submit" {
            return Err(NotaDecodeError::UnknownVariant {
                enum_name: "NotaSubmit",
                variant: head.to_owned(),
            });
        }
        let text = String::from_nota_block(&children[1])?;
        Ok(Self { text })
    }
}

#[cfg(feature = "nota-text")]
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
struct NotaInbox {
    name: String,
}

#[cfg(feature = "nota-text")]
impl RequestPayload for NotaInbox {}

#[cfg(feature = "nota-text")]
impl NotaEncode for NotaInbox {
    fn to_nota(&self) -> String {
        Delimiter::Parenthesis.wrap(["Inbox".to_owned(), self.name.to_nota()])
    }
}

#[cfg(feature = "nota-text")]
impl NotaDecode for NotaInbox {
    fn from_nota_block(block: &Block) -> Result<Self, NotaDecodeError> {
        let children = NotaBlock::new(block).expect_children(Delimiter::Parenthesis, "Inbox", 2)?;
        let head = children[0]
            .demote_to_string()
            .ok_or(NotaDecodeError::ExpectedAtom { type_name: "Inbox" })?;
        if head != "Inbox" {
            return Err(NotaDecodeError::UnknownVariant {
                enum_name: "NotaInbox",
                variant: head.to_owned(),
            });
        }
        let name = String::from_nota_block(&children[1])?;
        Ok(Self { name })
    }
}

#[cfg(feature = "nota-text")]
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
enum NotaChannelRequest {
    Submit(NotaSubmit),
    Inbox(NotaInbox),
}

#[cfg(feature = "nota-text")]
impl RequestPayload for NotaChannelRequest {}

#[cfg(feature = "nota-text")]
impl NotaEncode for NotaChannelRequest {
    fn to_nota(&self) -> String {
        match self {
            Self::Submit(payload) => payload.to_nota(),
            Self::Inbox(payload) => payload.to_nota(),
        }
    }
}

#[cfg(feature = "nota-text")]
impl NotaDecode for NotaChannelRequest {
    fn from_nota_block(block: &Block) -> Result<Self, NotaDecodeError> {
        let children = NotaBlock::new(block).expect_children(
            Delimiter::Parenthesis,
            "NotaChannelRequest",
            2,
        )?;
        let head = children[0]
            .demote_to_string()
            .ok_or(NotaDecodeError::ExpectedAtom {
                type_name: "NotaChannelRequest",
            })?;
        match head {
            "Submit" => Ok(Self::Submit(NotaSubmit::from_nota_block(block)?)),
            "Inbox" => Ok(Self::Inbox(NotaInbox::from_nota_block(block)?)),
            other => Err(NotaDecodeError::UnknownVariant {
                enum_name: "NotaChannelRequest",
                variant: other.to_string(),
            }),
        }
    }
}

#[cfg(feature = "nota-text")]
fn encode_to_text<T: NotaEncode>(value: &T) -> String {
    value.to_nota()
}

#[cfg(feature = "nota-text")]
fn decode_request_from_text(text: &str) -> Result<Request<NotaChannelRequest>, NotaDecodeError> {
    NotaSource::new(text).parse::<Request<NotaChannelRequest>>()
}

#[test]
#[cfg(feature = "nota-text")]
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
#[cfg(feature = "nota-text")]
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
