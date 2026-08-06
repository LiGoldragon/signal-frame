#![cfg(feature = "dotos-text")]

use dotos::{DotosDecode, DotosEncode, DotosSource};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_frame::{
    ContractBinding, ContractId, ExchangeFrameBody, ExchangeIdentifier, ExchangeLane, LaneSequence,
    NonEmpty, ObservableSet, RequestPayload, RootCode, SessionEpoch, StreamingFrameBody, SubReply,
    VariantCode, WireContract, WireRevision, WireRoute, signal_channel,
};
use std::num::{NonZeroU16, NonZeroU32};

struct TestContract;

impl WireContract for TestContract {
    const BINDING: ContractBinding = ContractBinding::new(
        ContractId::new(NonZeroU32::MIN),
        WireRevision::new(NonZeroU16::MIN),
    );
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct Submission {
    body: String,
}

impl Submission {
    fn new(body: impl Into<String>) -> Self {
        Self { body: body.into() }
    }
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct InboxQuery {
    name: String,
}

impl InboxQuery {
    fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct Receipt {
    accepted: bool,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct Inbox {
    count: u64,
}

mod message {
    use super::*;

    signal_channel! {
        channel Message contract TestContract {
            operation Submit(Submission),
            operation Query(InboxQuery),
        }
        reply Reply {
            Accepted(Receipt),
            Inbox(Inbox),
        }
    }
}

mod meta_message {
    use super::*;

    signal_channel! {
        channel MetaMessage contract TestContract {
            operation Configure(Submission),
            operation Drain(InboxQuery),
        }
        reply Reply {
            Configured(Receipt),
            Drained(Inbox),
        }
    }
}

mod unit_reply {
    use super::*;

    signal_channel! {
        channel UnitReply contract TestContract {
            operation Submit(Submission),
        }
        reply Reply {
            Accepted(Receipt),
            NotFound,
        }
    }
}

use message::{
    Frame as MessageFrame, Operation as MessageOperation, OperationKind as MessageOperationKind,
    Reply as MessageReply,
};
use meta_message::Operation as MetaMessageOperation;
use std::future::Future;
use std::task::{Context, Poll, Waker};

signal_frame::signal_cli! {
    pub struct MessageCommandLineDispatch {
        working message::Operation;
        meta meta_message::Operation;
    }
}

fn fresh_exchange() -> ExchangeIdentifier {
    ExchangeIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Connector,
        LaneSequence::first(),
    )
}

fn encode_to_text<T: DotosEncode>(value: &T) -> String {
    value.to_dotos()
}

fn block_on_ready<Output>(future: impl Future<Output = Output>) -> Output {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = Box::pin(future);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("test future unexpectedly yielded"),
    }
}

#[derive(Debug, thiserror::Error)]
enum MessageDispatchError {
    #[error(transparent)]
    Dispatch(#[from] signal_frame::OperationDispatchError),
    #[error(transparent)]
    Frame(#[from] signal_frame::FrameError),
}

#[derive(Default)]
struct MessageHandler {
    handled: Vec<MessageOperationKind>,
}

impl message::OperationHandler for MessageHandler {
    type Error = MessageDispatchError;

    async fn handle_validated(
        &mut self,
        operation: message::ValidatedOperation,
    ) -> Result<MessageReply, Self::Error> {
        match operation.into_operation() {
            MessageOperation::Submit(_) => {
                self.handled.push(MessageOperationKind::Submit);
                Ok(MessageReply::Accepted(Receipt { accepted: true }))
            }
            MessageOperation::Query(_) => {
                self.handled.push(MessageOperationKind::Query);
                Ok(MessageReply::Inbox(Inbox { count: 1 }))
            }
        }
    }
}

fn request_frame(route: WireRoute, operation: MessageOperation) -> MessageFrame {
    MessageFrame::new(
        route,
        ExchangeFrameBody::Request {
            exchange: fresh_exchange(),
            request: operation.into_request(),
        },
    )
}

#[test]
fn macro_emits_contract_local_operation_enum_without_signal_verb() {
    let operation = MessageOperation::Submit(Submission::new("hello"));

    assert_eq!(operation.kind(), MessageOperationKind::Submit);

    let request = operation.into_request();
    assert_eq!(request.payloads().len(), 1);
    assert_eq!(encode_to_text(&request), "(Submit {hello})");
}

#[test]
fn macro_request_text_round_trips_through_contract_local_heads() {
    let request = signal_frame::Request::from_payloads(NonEmpty::from_head_and_tail(
        MessageOperation::Submit(Submission::new("first")),
        vec![MessageOperation::Query(InboxQuery::new("operator"))],
    ));
    let text = encode_to_text(&request);

    assert_eq!(text, "[(Submit {first}) (Query {operator})]");

    let decoded = DotosSource::new(&text)
        .parse::<signal_frame::Request<MessageOperation>>()
        .expect("decode");
    assert_eq!(decoded, request);
}

#[test]
fn macro_unit_reply_round_trips_as_bare_dotos_atom() {
    let reply = unit_reply::Reply::NotFound;
    assert_eq!(reply.kind(), unit_reply::ReplyKind::NotFound);
    assert_eq!(reply.to_dotos(), "NotFound");

    let decoded = DotosSource::new("NotFound")
        .parse::<unit_reply::Reply>()
        .expect("decode bare unit reply");
    assert_eq!(decoded, reply);
}

#[test]
fn macro_unit_reply_round_trips_through_binary_frame() {
    let frame = unit_reply::Frame::new(
        WireRoute::new(RootCode::new(0), VariantCode::new(0)),
        ExchangeFrameBody::Reply {
            exchange: fresh_exchange(),
            reply: signal_frame::Reply::committed(NonEmpty::single(SubReply::Ok(
                unit_reply::Reply::NotFound,
            ))),
        },
    );

    let bytes = frame.encode_length_prefixed().expect("encode unit reply");
    let decoded = unit_reply::Frame::decode_length_prefixed(&bytes).expect("decode unit reply");
    let ExchangeFrameBody::Reply { reply, .. } = decoded.into_body() else {
        panic!("expected reply body");
    };
    let signal_frame::Reply::Accepted { per_operation, .. } = reply else {
        panic!("expected accepted reply");
    };
    assert_eq!(
        per_operation.into_head(),
        SubReply::Ok(unit_reply::Reply::NotFound)
    );
}

#[test]
fn macro_emits_static_operation_heads_for_cli_dispatch() {
    assert_eq!(
        <MessageOperation as signal_frame::SignalOperationHeads>::HEADS,
        &["Submit", "Query"]
    );
    assert_eq!(
        <MetaMessageOperation as signal_frame::SignalOperationHeads>::HEADS,
        &["Configure", "Drain"]
    );
}

#[test]
fn macro_emits_log_variant_for_operation_short_headers() {
    let submit = MessageOperation::Submit(Submission::new("hello"));
    let query = MessageOperation::Query(InboxQuery::new("operator"));

    assert_eq!(signal_frame::LogVariant::log_variant(&submit), 0);
    assert_eq!(signal_frame::LogVariant::log_variant(&query), 1);
    assert_eq!(
        submit.into_request().route().unwrap(),
        WireRoute::new(RootCode::new(0), VariantCode::new(0))
    );
    assert_eq!(
        query.into_request().route().unwrap(),
        WireRoute::new(RootCode::new(1), VariantCode::new(0))
    );
}

#[test]
fn macro_emits_operation_dispatch_handler_surface() {
    use message::OperationDispatch;

    let mut handler = MessageHandler::default();

    let reply = block_on_ready(handler.dispatch(request_frame(
        WireRoute::new(RootCode::new(0), VariantCode::new(0)),
        MessageOperation::Submit(Submission::new("hello")),
    )))
    .expect("dispatch");

    assert_eq!(handler.handled, vec![MessageOperationKind::Submit]);
    assert_eq!(reply, MessageReply::Accepted(Receipt { accepted: true }));

    let mismatch = block_on_ready(handler.dispatch(request_frame(
        WireRoute::new(RootCode::new(1), VariantCode::new(0)),
        MessageOperation::Submit(Submission::new("wrong header")),
    )))
    .expect_err("mismatched short header is rejected");
    assert!(matches!(
        mismatch,
        MessageDispatchError::Dispatch(
            signal_frame::OperationDispatchError::HeaderOperationMismatch {
                expected: 1,
                actual: 0
            }
        )
    ));

    let unknown = block_on_ready(handler.dispatch(request_frame(
        WireRoute::new(RootCode::new(99), VariantCode::new(0)),
        MessageOperation::Submit(Submission::new("unknown header")),
    )))
    .expect_err("unknown short header is rejected");
    assert!(matches!(
        unknown,
        MessageDispatchError::Dispatch(
            signal_frame::OperationDispatchError::UnknownOperationRoot { root: 99 }
        )
    ));
}

#[test]
fn operation_dispatch_rejects_forged_variant_before_handler() {
    use message::OperationDispatch;

    let mut handler = MessageHandler::default();
    let forged = block_on_ready(handler.dispatch(request_frame(
        WireRoute::new(RootCode::new(0), VariantCode::new(9)),
        MessageOperation::Submit(Submission::new("forged route")),
    )))
    .expect_err("nonzero route variant is rejected before dispatch");

    assert!(matches!(
        forged,
        MessageDispatchError::Dispatch(
            signal_frame::OperationDispatchError::UnknownOperationVariant {
                root: 0,
                variant: 9,
            }
        )
    ));
    assert!(handler.handled.is_empty(), "forged route reached handler");
}

#[test]
fn generated_surface_has_no_option_route_classifier() {
    let source = include_str!("../macros/src/emit.rs");
    assert!(!source.contains("pub fn kind_from_short_header"));
    assert!(!source.contains("Option<#kind_name>"));
}

#[test]
fn signal_cli_macro_routes_heads_to_working_or_meta_socket() {
    let dispatch = MessageCommandLineDispatch::new();

    assert_eq!(
        dispatch.route_head("Submit").expect("working head routes"),
        signal_frame::CommandLineSocket::Working
    );
    assert_eq!(
        dispatch.route_head("Drain").expect("meta head routes"),
        signal_frame::CommandLineSocket::Meta
    );
    assert!(matches!(
        dispatch.route_head("Unknown"),
        Err(signal_frame::CommandLineRouteError::UnknownRequestHead { .. })
    ));
}

#[test]
fn macro_frame_alias_round_trips_with_generated_payloads() {
    let exchange = fresh_exchange();
    let request = MessageOperation::Submit(Submission::new("frame")).into_request();
    let frame = MessageFrame::new(
        request.route().unwrap(),
        ExchangeFrameBody::Request {
            exchange,
            request: request.clone(),
        },
    );

    let bytes = frame.encode_length_prefixed().expect("encode");
    let decoded = MessageFrame::decode_length_prefixed(&bytes).expect("decode");

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

// These records model the shape that strict generated Interfaces expose:
// allocated text nested beneath products and sequences, with the validation
// and rancor bounds made explicit at the record boundary. The channel roots
// must propagate those generic bounds rather than assuming universally
// checkable leaf payloads.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct StrictReference {
    digest: String,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
#[rkyv(serialize_bounds(
    __S: rkyv::ser::Writer + rkyv::ser::Allocator,
    __S::Error: rkyv::rancor::Source,
))]
#[rkyv(deserialize_bounds(__D::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext)))]
pub struct StrictResolutionRequest {
    references: Vec<StrictReference>,
    model_path: Vec<String>,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
#[rkyv(serialize_bounds(
    __S: rkyv::ser::Writer + rkyv::ser::Allocator,
    __S::Error: rkyv::rancor::Source,
))]
#[rkyv(deserialize_bounds(__D::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext)))]
pub struct StrictResolution {
    accepted: Vec<StrictReference>,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
#[rkyv(serialize_bounds(
    __S: rkyv::ser::Writer + rkyv::ser::Allocator,
    __S::Error: rkyv::rancor::Source,
))]
#[rkyv(deserialize_bounds(__D::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext)))]
pub struct StrictResolutionUpdate {
    observed: Vec<StrictReference>,
    explanation: String,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct StrictLifecycleToken {
    value: u64,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct StrictLifecycleOpened {
    token: StrictLifecycleToken,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct StrictLifecycleClosed {
    token: StrictLifecycleToken,
}

mod strict_nested {
    use super::*;

    signal_channel! {
        channel StrictNested contract TestContract {
            operation Resolve(StrictResolutionRequest) opens ResolutionLifecycle,
            operation Close(StrictLifecycleToken),
        }
        reply Reply {
            Resolved(StrictResolution),
            Opened(StrictLifecycleOpened),
            Closed(StrictLifecycleClosed),
        }
        event Event {
            Updated(StrictResolutionUpdate) belongs ResolutionLifecycle,
        }
        stream ResolutionLifecycle {
            token StrictLifecycleToken;
            opened Opened;
            event Updated;
            close Close;
        }
    }
}

fn strict_reference(digest: &str) -> StrictReference {
    StrictReference {
        digest: digest.to_owned(),
    }
}

#[test]
fn macro_round_trips_nested_strict_request_reply_and_event_roots() {
    let exchange = fresh_exchange();
    let operation = strict_nested::Operation::Resolve(StrictResolutionRequest {
        references: vec![strict_reference("workflow"), strict_reference("operation")],
        model_path: vec!["provider".to_owned(), "model".to_owned()],
    });
    let request = operation.into_request();
    let route = request.route().expect("one operation has one route");
    assert_eq!(route, WireRoute::new(RootCode::new(0), VariantCode::new(0)));
    let request_text = request.to_dotos();
    let decoded_text = DotosSource::new(&request_text)
        .parse::<signal_frame::Request<strict_nested::Operation>>()
        .expect("decode nested strict request Dotos");
    assert_eq!(decoded_text, request);

    let frame = strict_nested::Frame::new(
        route,
        StreamingFrameBody::Request {
            exchange,
            request: request.clone(),
        },
    );
    let bytes = frame
        .encode_length_prefixed()
        .expect("encode nested strict request frame");
    let decoded = strict_nested::Frame::decode_length_prefixed(&bytes)
        .expect("decode nested strict request frame");
    assert_eq!(decoded.short_header().route(), route);
    assert_eq!(decoded.into_body(), frame.into_body());

    let reply = strict_nested::Reply::Resolved(StrictResolution {
        accepted: vec![strict_reference("workflow")],
    });
    let reply_text = reply.to_dotos();
    assert_eq!(
        DotosSource::new(&reply_text)
            .parse::<strict_nested::Reply>()
            .expect("decode nested strict reply Dotos"),
        reply,
    );

    let event = strict_nested::Event::Updated(StrictResolutionUpdate {
        observed: vec![strict_reference("workflow"), strict_reference("operation")],
        explanation: "authority accepted the resolution".to_owned(),
    });
    let event_text = event.to_dotos();
    assert_eq!(
        DotosSource::new(&event_text)
            .parse::<strict_nested::Event>()
            .expect("decode nested strict event Dotos"),
        event,
    );
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct WatchWorker {
    name: String,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct WorkerToken {
    number: u64,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct WorkerOpened {
    number: u64,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct WorkerStopped {
    number: u64,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct WorkerStarted {
    number: u64,
}

mod terminal {
    use super::*;

    signal_channel! {
        channel Terminal contract TestContract {
            operation Watch(WatchWorker) opens WorkerLifecycle,
            operation Stop(WorkerToken),
        }
        reply Reply {
            Opened(WorkerOpened),
            Stopped(WorkerStopped),
        }
        event Event {
            Started(WorkerStarted) belongs WorkerLifecycle,
        }
        stream WorkerLifecycle {
            token WorkerToken;
            opened Opened;
            event Started;
            close Stop;
        }
    }
}

use terminal::{
    Event as TerminalEvent, Frame as TerminalFrame, Operation as TerminalOperation,
    StreamKind as TerminalStreamKind,
};

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
    let event_identifier =
        signal_frame::StreamEventIdentifier::acceptor(SessionEpoch::new(1), LaneSequence::first());
    let frame = TerminalFrame::new(
        WireRoute::new(RootCode::new(0), VariantCode::new(0)),
        StreamingFrameBody::SubscriptionEvent {
            event_identifier,
            token: signal_frame::SubscriptionTokenInner::new(5),
            event: TerminalEvent::Started(WorkerStarted { number: 5 }),
        },
    );

    let bytes = frame.encode_length_prefixed().expect("encode");
    let decoded = TerminalFrame::decode_length_prefixed(&bytes).expect("decode");

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

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub enum LedgerObserverFilter {
    All,
    OnlyOperations,
    OnlyEffects,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct LedgerNote {
    body: String,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct LedgerAcknowledgement {
    accepted: bool,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct OperationReceived {
    operation_kind: String,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct EffectEmitted {
    effect_label: String,
}

mod ledger {
    use super::*;

    signal_channel! {
        channel Ledger contract TestContract {
            operation Record(LedgerNote),
        }
        reply Reply {
            Recorded(LedgerAcknowledgement),
        }
        observable {
            filter LedgerObserverFilter;
            operation_event OperationReceived;
            effect_event EffectEmitted;
        }
    }
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct Selection {
    name: String,
}

mod domain_observe {
    use super::*;

    signal_channel! {
        channel DomainObserve contract TestContract {
            operation Observe(Selection),
        }
        reply Reply {
            Observed(LedgerAcknowledgement),
        }
        observable {
            filter LedgerObserverFilter;
            operation_event OperationReceived;
            effect_event EffectEmitted;
        }
    }
}

use domain_observe::{
    Operation as DomainObserveOperation, OperationKind as DomainObserveOperationKind,
    StreamKind as DomainObserveStreamKind,
};
use ledger::{
    Event as LedgerEvent, Frame as LedgerFrame, ObserverSet as LedgerObserverSet,
    ObserverSubscriptionOpened as LedgerObserverSubscriptionOpened,
    ObserverSubscriptionToken as LedgerObserverSubscriptionToken, Operation as LedgerOperation,
    OperationKind as LedgerOperationKind, Reply as LedgerReply, ReplyKind as LedgerReplyKind,
    StreamKind as LedgerStreamKind,
};

impl domain_observe::ObserverFilterMatch for LedgerObserverFilter {
    fn matches_operation_received(&self, _event: &OperationReceived) -> bool {
        matches!(self, Self::All | Self::OnlyOperations)
    }

    fn matches_effect_emitted(&self, _event: &EffectEmitted) -> bool {
        matches!(self, Self::All | Self::OnlyEffects)
    }
}

impl ledger::ObserverFilterMatch for LedgerObserverFilter {
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
fn observable_round_trips_tap_and_untap_through_dotos() {
    let tap_request = LedgerOperation::Tap(LedgerObserverFilter::OnlyOperations).into_request();
    let tap_text = encode_to_text(&tap_request);
    assert_eq!(tap_text, "(Tap OnlyOperations)");

    let decoded = DotosSource::new(&tap_text)
        .parse::<signal_frame::Request<LedgerOperation>>()
        .expect("decode tap");
    assert_eq!(decoded, tap_request);

    let token = LedgerObserverSubscriptionToken::new(signal_frame::SubscriptionTokenInner::new(9));
    let untap_request = LedgerOperation::Untap(token).into_request();
    let untap_text = encode_to_text(&untap_request);
    assert_eq!(untap_text, "(Untap (ObserverSubscriptionToken 9))");

    let decoded_untap = DotosSource::new(&untap_text)
        .parse::<signal_frame::Request<LedgerOperation>>()
        .expect("decode untap");
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

    let decoded = DotosSource::new(&text)
        .parse::<LedgerReply>()
        .expect("decode opened");
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
    // author's DotosEncode, DotosDecode-derived struct of the same name.
    let decoded = DotosSource::new(&text)
        .parse::<LedgerEvent>()
        .expect("decode event");
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

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct AuditOperationReceived {
    label: String,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct AuditEffectEmitted {
    label: String,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct AuditProbe {
    target: String,
}

#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, DotosEncode, DotosDecode, Debug, Clone, PartialEq, Eq,
)]
pub struct AuditProbed {
    accepted: bool,
}

mod audit {
    use super::*;

    signal_channel! {
        channel Audit contract TestContract {
            operation Probe(AuditProbe),
        }
        reply Reply {
            Probed(AuditProbed),
        }
        observable {
            filter default;
            operation_event AuditOperationReceived;
            effect_event AuditEffectEmitted;
        }
    }
}

use audit::{
    ObserverFilter as AuditObserverFilter, ObserverFilterMatch as _,
    ObserverSet as AuditObserverSet, ObserverSubscriptionToken as AuditObserverSubscriptionToken,
    Operation as AuditOperation,
};

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

        let decoded = DotosSource::new(&text)
            .parse::<signal_frame::Request<AuditOperation>>()
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
    let event_identifier =
        signal_frame::StreamEventIdentifier::acceptor(SessionEpoch::new(1), LaneSequence::first());
    let event = LedgerEvent::EffectEmitted(EffectEmitted {
        effect_label: "Assert".to_string(),
    });
    let frame = LedgerFrame::new(
        WireRoute::new(RootCode::new(0), VariantCode::new(0)),
        StreamingFrameBody::SubscriptionEvent {
            event_identifier,
            token: signal_frame::SubscriptionTokenInner::new(11),
            event: event.clone(),
        },
    );

    let bytes = frame.encode_length_prefixed().expect("encode");
    let decoded = LedgerFrame::decode_length_prefixed(&bytes).expect("decode");

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
