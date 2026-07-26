#![cfg(feature = "nota-text")]

use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use nota::{NotaDecode, NotaEncode};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_frame::{
    ClientShape, CommandLineError, CommandLineSocket, CommandLineSockets, ContractBinding,
    ContractId, ExchangeFrameBody, ExchangeIdentifier, NonEmpty, ProcessIdentifier, Reply,
    RequestHead, SignalOperationHeads, SingleArgument, SingleArgumentError, SubReply, WireContract,
    WireRevision,
};
use std::num::{NonZeroU16, NonZeroU32};

struct TestContract;

impl WireContract for TestContract {
    const BINDING: ContractBinding = ContractBinding::new(
        ContractId::new(NonZeroU32::MIN),
        WireRevision::new(NonZeroU16::MIN),
    );
}

mod working {
    use super::*;

    #[derive(
        Archive, RkyvSerialize, RkyvDeserialize, NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq,
    )]
    pub struct Submission {
        body: String,
    }

    impl Submission {
        pub fn new(body: impl Into<String>) -> Self {
            Self { body: body.into() }
        }
    }

    #[derive(
        Archive, RkyvSerialize, RkyvDeserialize, NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq,
    )]
    pub struct Query {
        selection: String,
    }

    #[derive(
        Archive, RkyvSerialize, RkyvDeserialize, NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq,
    )]
    pub struct Accepted {
        ok: bool,
    }

    impl Accepted {
        pub const fn new(ok: bool) -> Self {
            Self { ok }
        }
    }

    signal_frame::signal_channel! {
        channel Working contract TestContract {
            operation Submit(Submission),
            operation Query(Query),
        }
        reply Reply {
            Accepted(Accepted),
        }
    }
}

mod meta {
    use super::*;

    #[derive(
        Archive, RkyvSerialize, RkyvDeserialize, NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq,
    )]
    pub struct Drain {}

    #[derive(
        Archive, RkyvSerialize, RkyvDeserialize, NotaEncode, NotaDecode, Debug, Clone, PartialEq, Eq,
    )]
    pub struct Drained {}

    signal_frame::signal_channel! {
        channel Meta contract TestContract {
            operation Drain(Drain),
        }
        reply Reply {
            Drained(Drained),
        }
    }
}

fn encode_to_text<T: NotaEncode>(value: &T) -> String {
    value.to_nota()
}

fn socket_path(name: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "signal-frame-{name}-{}-{timestamp}.sock",
        std::process::id()
    ))
}

fn read_frame(stream: &mut UnixStream) -> working::Frame {
    let mut prefix = [0_u8; 4];
    stream.read_exact(&mut prefix).expect("read prefix");
    let length = u32::from_be_bytes(prefix) as usize;
    let mut bytes = Vec::with_capacity(4 + length);
    bytes.extend_from_slice(&prefix);
    bytes.resize(4 + length, 0);
    stream.read_exact(&mut bytes[4..]).expect("read body");
    working::Frame::decode_length_prefixed(&bytes).expect("decode frame")
}

fn write_frame(stream: &mut UnixStream, frame: &working::Frame) {
    let bytes = frame.encode_length_prefixed().expect("encode reply");
    stream.write_all(&bytes).expect("write reply");
    stream.flush().expect("flush reply");
}

#[test]
fn single_argument_accepts_exactly_one_non_flag_argument() {
    let argument = SingleArgument::from_arguments(["spirit".to_string(), "(Observe)".to_string()])
        .expect("one argument accepted");

    assert_eq!(argument.as_str(), "(Observe)");

    assert!(matches!(
        SingleArgument::from_arguments(["spirit".to_string()]),
        Err(SingleArgumentError::WrongArgumentCount { found: 0, .. })
    ));
    assert!(matches!(
        SingleArgument::from_arguments(["spirit".to_string(), "--help".to_string()]),
        Err(SingleArgumentError::FlagArgument { .. })
    ));
}

#[test]
fn request_head_routes_first_payload_in_sequence() {
    let head =
        RequestHead::from_text("[(Submit {hello}) (Query {everything})]").expect("head parsed");

    assert_eq!(
        head.route::<working::Operation, meta::Operation>()
            .expect("route"),
        CommandLineSocket::Working
    );
    assert_eq!(
        <working::Operation as SignalOperationHeads>::HEADS,
        &["Submit", "Query"]
    );
}

#[test]
fn command_line_sockets_derive_persona_environment_names_from_binary_name() {
    let sockets = CommandLineSockets::from_binary_name("spirit");

    assert_eq!(sockets.working_variable(), "PERSONA_SPIRIT_SOCKET");
    assert_eq!(sockets.meta_variable(), "PERSONA_SPIRIT_META_SOCKET");

    let sockets = CommandLineSockets::from_binary_name("persona_orchestrate");

    assert_eq!(sockets.working_variable(), "PERSONA_ORCHESTRATE_SOCKET");
    assert_eq!(sockets.meta_variable(), "PERSONA_ORCHESTRATE_META_SOCKET");
}

#[test]
fn client_shape_sends_request_with_caller_and_prints_reply() {
    let socket = socket_path("client-shape");
    let listener = UnixListener::bind(&socket).expect("bind listener");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept client");
        let frame = read_frame(&mut stream);
        let ExchangeFrameBody::Request { exchange, request } = frame.into_body() else {
            panic!("expected request frame");
        };
        let caller = request.caller().expect("caller captured");
        assert_ne!(caller.pid, ProcessIdentifier::new(1));
        assert_eq!(
            request.payloads().head(),
            &working::Operation::Submit(working::Submission::new("hello"))
        );

        let reply = working::Frame::new(
            signal_frame::WireRoute::new(
                signal_frame::RootCode::new(0),
                signal_frame::VariantCode::new(0),
            ),
            ExchangeFrameBody::Reply {
                exchange,
                reply: Reply::committed(NonEmpty::single(SubReply::Ok(working::Reply::Accepted(
                    working::Accepted::new(true),
                )))),
            },
        );
        write_frame(&mut stream, &reply);
    });

    let argument =
        SingleArgument::from_arguments(["spirit".to_string(), "(Submit {hello})".to_string()])
            .expect("argument");
    let client = ClientShape::<working::Frame, meta::Frame>::new(CommandLineSockets::working_only(
        socket.clone(),
    ));
    let reply = client.reply_text(argument).expect("reply text");

    assert_eq!(
        reply,
        encode_to_text(&working::Reply::Accepted(working::Accepted::new(true)))
    );

    server.join().expect("server joins");
    let _ = std::fs::remove_file(socket);
}

#[test]
fn client_shape_rejects_reply_for_a_different_exchange() {
    let socket = socket_path("client-shape-exchange-mismatch");
    let listener = UnixListener::bind(&socket).expect("bind listener");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept client");
        let frame = read_frame(&mut stream);
        let ExchangeFrameBody::Request { exchange, .. } = frame.into_body() else {
            panic!("expected request frame");
        };
        let wrong_exchange = ExchangeIdentifier::new(
            exchange.session_epoch(),
            exchange.lane(),
            exchange.sequence().next(),
        );
        let reply = working::Frame::new(
            signal_frame::WireRoute::new(
                signal_frame::RootCode::new(0),
                signal_frame::VariantCode::new(0),
            ),
            ExchangeFrameBody::Reply {
                exchange: wrong_exchange,
                reply: Reply::committed(NonEmpty::single(SubReply::Ok(working::Reply::Accepted(
                    working::Accepted::new(true),
                )))),
            },
        );
        write_frame(&mut stream, &reply);
    });

    let argument =
        SingleArgument::from_arguments(["spirit".to_string(), "(Submit {hello})".to_string()])
            .unwrap();
    let client = ClientShape::<working::Frame, meta::Frame>::new(CommandLineSockets::working_only(
        socket.clone(),
    ));
    let error = client.reply_text(argument).unwrap_err();
    assert!(matches!(
        error,
        CommandLineError::ReplyExchangeMismatch {
            expected,
            found,
        } if expected.sequence().next() == found.sequence()
    ));

    server.join().expect("server joins");
    let _ = std::fs::remove_file(socket);
}

#[test]
fn client_shape_prints_multi_operation_replies_as_sequence() {
    let socket = socket_path("client-shape-sequence");
    let listener = UnixListener::bind(&socket).expect("bind listener");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept client");
        let frame = read_frame(&mut stream);
        let ExchangeFrameBody::Request { exchange, request } = frame.into_body() else {
            panic!("expected request frame");
        };
        assert!(request.caller().is_some());
        assert_eq!(request.payloads().len(), 2);

        let reply = working::Frame::new(
            signal_frame::WireRoute::new(
                signal_frame::RootCode::new(0),
                signal_frame::VariantCode::new(0),
            ),
            ExchangeFrameBody::Reply {
                exchange,
                reply: Reply::committed(NonEmpty::from_head_and_tail(
                    SubReply::Ok(working::Reply::Accepted(working::Accepted::new(true))),
                    vec![SubReply::Ok(working::Reply::Accepted(
                        working::Accepted::new(false),
                    ))],
                )),
            },
        );
        write_frame(&mut stream, &reply);
    });

    let argument = SingleArgument::from_arguments([
        "spirit".to_string(),
        "[(Submit {hello}) (Submit {again})]".to_string(),
    ])
    .expect("argument");
    let client = ClientShape::<working::Frame, meta::Frame>::new(CommandLineSockets::working_only(
        socket.clone(),
    ));
    let reply = client.reply_text(argument).expect("reply text");

    assert_eq!(
        reply,
        format!(
            "[{} {}]",
            encode_to_text(&working::Reply::Accepted(working::Accepted::new(true))),
            encode_to_text(&working::Reply::Accepted(working::Accepted::new(false)))
        )
    );

    server.join().expect("server joins");
    let _ = std::fs::remove_file(socket);
}
