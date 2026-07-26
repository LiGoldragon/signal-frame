use std::{
    fmt::Debug,
    fs,
    io::{Read, Write},
    marker::PhantomData,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::ExitCode,
};

use nota::{Block, Delimiter, Document, NotaDecode, NotaEncode, NotaSource};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use thiserror::Error;

use crate::{
    BoundExchangeFrame, BoundStreamingFrame, Caller, ExchangeFrameBody, ExchangeIdentifier,
    ExchangeLane, LaneSequence, LogVariant, NonEmpty, Reply, Request, RequestPayload, SessionEpoch,
    SignalOperationHeads, StreamingFrameBody, SubReply, WireContract, WireRouteError,
};

type HighSerializer<'archive> = rkyv::api::high::HighSerializer<
    rkyv::util::AlignedVec,
    rkyv::ser::allocator::ArenaHandle<'archive>,
    rkyv::rancor::Error,
>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLineSocket {
    Working,
    Meta,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandLineRouteError {
    #[error("unknown request head: {head}")]
    UnknownRequestHead { head: String },

    #[error("request head appears in both working and meta contracts: {head}")]
    AmbiguousRequestHead { head: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SingleArgumentError {
    #[error("{program} expects exactly one NOTA or signal-file argument, found {found}")]
    WrongArgumentCount { program: String, found: usize },

    #[error("{program} accepts NOTA or signal-file input, not flag-style argument {argument}")]
    FlagArgument { program: String, argument: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandLineError {
    #[error("{0}")]
    Argument(#[from] SingleArgumentError),

    #[error("missing socket environment variable {variable}")]
    MissingSocket { variable: String },

    #[error("cannot route command-line request: {reason}")]
    Route { reason: String },

    #[error("invalid request text: {reason}")]
    InvalidRequest { reason: String },

    #[error("invalid reply text: {reason}")]
    InvalidReply { reason: String },

    #[error("input/output error: {reason}")]
    InputOutput { reason: String },

    #[error("signal frame error: {reason}")]
    SignalFrame { reason: String },

    #[error("frame too large: found {found} bytes, limit {limit}")]
    FrameTooLarge { found: usize, limit: usize },

    #[error("unexpected signal frame: expected {expected}, got {got}")]
    UnexpectedFrame { expected: &'static str, got: String },

    #[error("request rejected before execution: {reason}")]
    RequestRejected { reason: String },

    #[error("request route cannot be represented in the short header: {0}")]
    InvalidRoute(#[from] WireRouteError),

    #[error("reply exchange mismatch: expected {expected:?}, found {found:?}")]
    ReplyExchangeMismatch {
        expected: ExchangeIdentifier,
        found: ExchangeIdentifier,
    },
}

impl CommandLineError {
    fn input_output(error: std::io::Error) -> Self {
        Self::InputOutput {
            reason: error.to_string(),
        }
    }

    fn signal_frame(error: impl std::fmt::Display) -> Self {
        Self::SignalFrame {
            reason: error.to_string(),
        }
    }

    fn invalid_request(error: impl std::fmt::Display) -> Self {
        Self::InvalidRequest {
            reason: error.to_string(),
        }
    }

    fn route(error: CommandLineRouteError) -> Self {
        Self::Route {
            reason: error.to_string(),
        }
    }

    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::Argument(_)
            | Self::Route { .. }
            | Self::InvalidRequest { .. }
            | Self::InvalidReply { .. } => ExitCode::from(64),
            Self::MissingSocket { .. } | Self::InputOutput { .. } => ExitCode::from(69),
            Self::SignalFrame { .. }
            | Self::FrameTooLarge { .. }
            | Self::UnexpectedFrame { .. }
            | Self::RequestRejected { .. }
            | Self::InvalidRoute(_)
            | Self::ReplyExchangeMismatch { .. } => ExitCode::from(70),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleArgument {
    value: String,
}

impl SingleArgument {
    pub fn from_arguments<I>(arguments: I) -> Result<Self, SingleArgumentError>
    where
        I: IntoIterator<Item = String>,
    {
        let mut iterator = arguments.into_iter();
        let program = iterator.next().unwrap_or_else(|| "signal-cli".to_string());
        let values: Vec<String> = iterator.collect();
        Self::from_program_and_values(program, values)
    }

    pub fn from_environment() -> Result<Self, SingleArgumentError> {
        Self::from_arguments(std::env::args())
    }

    pub fn from_program_and_values(
        program: String,
        values: Vec<String>,
    ) -> Result<Self, SingleArgumentError> {
        match values.as_slice() {
            [value] if value.starts_with("--") => Err(SingleArgumentError::FlagArgument {
                program,
                argument: value.clone(),
            }),
            [value] => Ok(Self {
                value: value.clone(),
            }),
            _ => Err(SingleArgumentError::WrongArgumentCount {
                program,
                found: values.len(),
            }),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub fn into_string(self) -> String {
        self.value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandLineRouteTable<'head> {
    working_heads: &'head [&'head str],
    meta_heads: &'head [&'head str],
}

impl<'head> CommandLineRouteTable<'head> {
    pub const fn new(working_heads: &'head [&'head str], meta_heads: &'head [&'head str]) -> Self {
        Self {
            working_heads,
            meta_heads,
        }
    }

    pub fn route_head(&self, head: &str) -> Result<CommandLineSocket, CommandLineRouteError> {
        match (
            self.working_heads.contains(&head),
            self.meta_heads.contains(&head),
        ) {
            (true, false) => Ok(CommandLineSocket::Working),
            (false, true) => Ok(CommandLineSocket::Meta),
            (true, true) => Err(CommandLineRouteError::AmbiguousRequestHead {
                head: head.to_string(),
            }),
            (false, false) => Err(CommandLineRouteError::UnknownRequestHead {
                head: head.to_string(),
            }),
        }
    }

    pub const fn working_heads(&self) -> &'head [&'head str] {
        self.working_heads
    }

    pub const fn meta_heads(&self) -> &'head [&'head str] {
        self.meta_heads
    }
}

pub struct CommandLineDispatch<Working, Meta> {
    table: CommandLineRouteTable<'static>,
    marker: PhantomData<fn() -> (Working, Meta)>,
}

impl<Working, Meta> CommandLineDispatch<Working, Meta>
where
    Working: SignalOperationHeads,
    Meta: SignalOperationHeads,
{
    pub const fn new() -> Self {
        Self {
            table: CommandLineRouteTable::new(Working::HEADS, Meta::HEADS),
            marker: PhantomData,
        }
    }

    pub fn route_head(&self, head: &str) -> Result<CommandLineSocket, CommandLineRouteError> {
        self.table.route_head(head)
    }

    pub const fn table(&self) -> CommandLineRouteTable<'static> {
        self.table
    }
}

impl<Working, Meta> Default for CommandLineDispatch<Working, Meta>
where
    Working: SignalOperationHeads,
    Meta: SignalOperationHeads,
{
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLineSockets {
    working_socket: Option<PathBuf>,
    meta_socket: Option<PathBuf>,
    working_variable: String,
    meta_variable: String,
}

impl CommandLineSockets {
    pub fn new(
        working_socket: Option<PathBuf>,
        meta_socket: Option<PathBuf>,
        working_variable: impl Into<String>,
        meta_variable: impl Into<String>,
    ) -> Self {
        Self {
            working_socket,
            meta_socket,
            working_variable: working_variable.into(),
            meta_variable: meta_variable.into(),
        }
    }

    pub fn from_binary_name(binary_name: &str) -> Self {
        let stem = EnvironmentStem::from_binary_name(binary_name);
        let working_variable = format!("{stem}_SOCKET");
        let meta_variable = format!("{stem}_META_SOCKET");
        Self {
            working_socket: std::env::var(&working_variable).ok().map(PathBuf::from),
            meta_socket: std::env::var(&meta_variable).ok().map(PathBuf::from),
            working_variable,
            meta_variable,
        }
    }

    pub fn working_only(socket: impl Into<PathBuf>) -> Self {
        Self::new(
            Some(socket.into()),
            None,
            "SIGNAL_WORKING_SOCKET",
            "SIGNAL_META_SOCKET",
        )
    }

    pub fn working_socket(&self) -> Result<&Path, CommandLineError> {
        self.working_socket
            .as_deref()
            .ok_or_else(|| CommandLineError::MissingSocket {
                variable: self.working_variable.clone(),
            })
    }

    pub fn meta_socket(&self) -> Result<&Path, CommandLineError> {
        self.meta_socket
            .as_deref()
            .ok_or_else(|| CommandLineError::MissingSocket {
                variable: self.meta_variable.clone(),
            })
    }

    pub fn working_variable(&self) -> &str {
        &self.working_variable
    }

    pub fn meta_variable(&self) -> &str {
        &self.meta_variable
    }
}

struct EnvironmentStem(String);

impl EnvironmentStem {
    fn from_binary_name(binary_name: &str) -> Self {
        let mut stem = String::from("PERSONA_");
        let binary_name = binary_name
            .strip_prefix("persona_")
            .or_else(|| binary_name.strip_prefix("persona-"))
            .unwrap_or(binary_name);
        let mut previous_separator = true;
        for character in binary_name.chars() {
            if character.is_ascii_alphanumeric() {
                stem.push(character.to_ascii_uppercase());
                previous_separator = false;
            } else if !previous_separator {
                stem.push('_');
                previous_separator = true;
            }
        }
        Self(stem.trim_end_matches('_').to_string())
    }
}

impl std::fmt::Display for EnvironmentStem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestInput {
    argument: SingleArgument,
}

impl RequestInput {
    pub fn new(argument: SingleArgument) -> Self {
        Self { argument }
    }

    pub fn text(&self) -> Result<String, CommandLineError> {
        let value = self.argument.as_str();
        let trimmed = value.trim_start();
        if trimmed.starts_with('(') || trimmed.starts_with('[') {
            Ok(value.to_string())
        } else {
            fs::read_to_string(value).map_err(CommandLineError::input_output)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHead {
    head: String,
}

impl RequestHead {
    pub fn from_text(text: &str) -> Result<Self, CommandLineError> {
        let document = Document::parse(text).map_err(CommandLineError::invalid_request)?;
        let block = document
            .root_object_at(0)
            .ok_or_else(|| CommandLineError::InvalidRequest {
                reason: "expected request object".to_owned(),
            })?;
        let head = RequestHeadBlock::new(block).head()?;
        Ok(Self {
            head: head.to_owned(),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.head
    }

    pub fn route<Working, Meta>(&self) -> Result<CommandLineSocket, CommandLineError>
    where
        Working: SignalOperationHeads,
        Meta: SignalOperationHeads,
    {
        CommandLineDispatch::<Working, Meta>::new()
            .route_head(self.as_str())
            .map_err(CommandLineError::route)
    }
}

struct RequestHeadBlock<'block> {
    block: &'block Block,
}

impl<'block> RequestHeadBlock<'block> {
    fn new(block: &'block Block) -> Self {
        Self { block }
    }

    fn head(&self) -> Result<&'block str, CommandLineError> {
        match self.block.as_delimited(Delimiter::SquareBracket) {
            Some(sequence) => sequence
                .first()
                .ok_or_else(|| CommandLineError::InvalidRequest {
                    reason: "expected at least one request in bracketed sequence".to_owned(),
                })
                .and_then(|block| Self::new(block).head()),
            None => self
                .block
                .root_object_at(0)
                .and_then(Block::demote_to_string)
                .or_else(|| self.block.demote_to_string())
                .ok_or_else(|| CommandLineError::InvalidRequest {
                    reason: "expected request record head".to_owned(),
                }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestText<Operation> {
    text: String,
    marker: PhantomData<fn() -> Operation>,
}

impl<Operation> RequestText<Operation> {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            marker: PhantomData,
        }
    }
}

impl<Operation> RequestText<Operation>
where
    Operation: NotaDecode,
{
    pub fn decode_request(&self) -> Result<Request<Operation>, CommandLineError> {
        NotaSource::new(&self.text)
            .parse::<Request<Operation>>()
            .map_err(CommandLineError::invalid_request)
    }
}

/// Frame shape usable by the generated command-line client.
pub trait ClientFrame: Sized {
    type Operation;
    type Reply;

    fn request_frame(
        exchange: ExchangeIdentifier,
        request: Request<Self::Operation>,
    ) -> Result<Self, CommandLineError>;
    fn reply_from_frame(
        self,
        expected_exchange: ExchangeIdentifier,
    ) -> Result<Reply<Self::Reply>, CommandLineError>;
    fn encode_client_frame(&self) -> Result<Vec<u8>, CommandLineError>;
    fn decode_client_frame(bytes: &[u8]) -> Result<Self, CommandLineError>;
}

impl<Contract, Operation, ReplyPayload> ClientFrame
    for BoundExchangeFrame<Contract, Operation, ReplyPayload>
where
    Contract: WireContract,
    Operation: Archive + Debug + LogVariant + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
    ReplyPayload: Archive + Debug + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
    <Operation as Archive>::Archived: for<'archive> rkyv::bytecheck::CheckBytes<
            rkyv::api::high::HighValidator<'archive, rkyv::rancor::Error>,
        > + RkyvDeserialize<Operation, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
    <ReplyPayload as Archive>::Archived: for<'archive> rkyv::bytecheck::CheckBytes<
            rkyv::api::high::HighValidator<'archive, rkyv::rancor::Error>,
        > + RkyvDeserialize<ReplyPayload, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
{
    type Operation = Operation;
    type Reply = ReplyPayload;

    fn request_frame(
        exchange: ExchangeIdentifier,
        request: Request<Self::Operation>,
    ) -> Result<Self, CommandLineError> {
        Ok(Self::new(
            request.route()?,
            ExchangeFrameBody::Request { exchange, request },
        ))
    }

    fn reply_from_frame(
        self,
        expected_exchange: ExchangeIdentifier,
    ) -> Result<Reply<Self::Reply>, CommandLineError> {
        match self.into_body() {
            ExchangeFrameBody::Reply { exchange, reply } if exchange == expected_exchange => {
                Ok(reply)
            }
            ExchangeFrameBody::Reply {
                exchange: found, ..
            } => Err(CommandLineError::ReplyExchangeMismatch {
                expected: expected_exchange,
                found,
            }),
            other => Err(CommandLineError::UnexpectedFrame {
                expected: "reply",
                got: format!("{other:?}"),
            }),
        }
    }

    fn encode_client_frame(&self) -> Result<Vec<u8>, CommandLineError> {
        self.encode_length_prefixed()
            .map_err(CommandLineError::signal_frame)
    }

    fn decode_client_frame(bytes: &[u8]) -> Result<Self, CommandLineError> {
        Self::decode_length_prefixed(bytes).map_err(CommandLineError::signal_frame)
    }
}

impl<Contract, Operation, ReplyPayload, EventPayload> ClientFrame
    for BoundStreamingFrame<Contract, Operation, ReplyPayload, EventPayload>
where
    Contract: WireContract,
    Operation: Archive + Debug + LogVariant + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
    ReplyPayload: Archive + Debug + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
    EventPayload: Archive + Debug + for<'archive> RkyvSerialize<HighSerializer<'archive>>,
    <Operation as Archive>::Archived: for<'archive> rkyv::bytecheck::CheckBytes<
            rkyv::api::high::HighValidator<'archive, rkyv::rancor::Error>,
        > + RkyvDeserialize<Operation, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
    <ReplyPayload as Archive>::Archived: for<'archive> rkyv::bytecheck::CheckBytes<
            rkyv::api::high::HighValidator<'archive, rkyv::rancor::Error>,
        > + RkyvDeserialize<ReplyPayload, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
    <EventPayload as Archive>::Archived: for<'archive> rkyv::bytecheck::CheckBytes<
            rkyv::api::high::HighValidator<'archive, rkyv::rancor::Error>,
        > + RkyvDeserialize<EventPayload, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
{
    type Operation = Operation;
    type Reply = ReplyPayload;

    fn request_frame(
        exchange: ExchangeIdentifier,
        request: Request<Self::Operation>,
    ) -> Result<Self, CommandLineError> {
        Ok(Self::new(
            request.route()?,
            StreamingFrameBody::Request { exchange, request },
        ))
    }

    fn reply_from_frame(
        self,
        expected_exchange: ExchangeIdentifier,
    ) -> Result<Reply<Self::Reply>, CommandLineError> {
        match self.into_body() {
            StreamingFrameBody::Reply { exchange, reply } if exchange == expected_exchange => {
                Ok(reply)
            }
            StreamingFrameBody::Reply {
                exchange: found, ..
            } => Err(CommandLineError::ReplyExchangeMismatch {
                expected: expected_exchange,
                found,
            }),
            other => Err(CommandLineError::UnexpectedFrame {
                expected: "reply",
                got: format!("{other:?}"),
            }),
        }
    }

    fn encode_client_frame(&self) -> Result<Vec<u8>, CommandLineError> {
        self.encode_length_prefixed()
            .map_err(CommandLineError::signal_frame)
    }

    fn decode_client_frame(bytes: &[u8]) -> Result<Self, CommandLineError> {
        Self::decode_length_prefixed(bytes).map_err(CommandLineError::signal_frame)
    }
}

pub struct ClientShape<Working, Meta> {
    sockets: CommandLineSockets,
    maximum_frame_bytes: usize,
    marker: PhantomData<fn() -> (Working, Meta)>,
}

impl<Working, Meta> ClientShape<Working, Meta> {
    pub const DEFAULT_MAXIMUM_FRAME_BYTES: usize = 8 * 1024 * 1024;

    pub fn new(sockets: CommandLineSockets) -> Self {
        Self {
            sockets,
            maximum_frame_bytes: Self::DEFAULT_MAXIMUM_FRAME_BYTES,
            marker: PhantomData,
        }
    }

    pub fn from_binary_name(binary_name: &str) -> Self {
        Self::new(CommandLineSockets::from_binary_name(binary_name))
    }

    pub fn with_maximum_frame_bytes(mut self, maximum_frame_bytes: usize) -> Self {
        self.maximum_frame_bytes = maximum_frame_bytes;
        self
    }
}

impl<Working, Meta> ClientShape<Working, Meta>
where
    Working: ClientFrame,
    Meta: ClientFrame,
    Working::Operation: SignalOperationHeads + RequestPayload + NotaDecode,
    Meta::Operation: SignalOperationHeads + RequestPayload + NotaDecode,
    Working::Reply: NotaEncode + Debug,
    Meta::Reply: NotaEncode + Debug,
{
    pub fn run_from_environment(binary_name: &str) -> ExitCode {
        match Self::from_binary_name(binary_name).run_environment() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                error.exit_code()
            }
        }
    }

    pub fn run_environment(&self) -> Result<(), CommandLineError> {
        let argument = SingleArgument::from_environment()?;
        self.run_argument(argument)
    }

    pub fn run_argument(&self, argument: SingleArgument) -> Result<(), CommandLineError> {
        println!("{}", self.reply_text(argument)?);
        Ok(())
    }

    pub fn reply_text(&self, argument: SingleArgument) -> Result<String, CommandLineError> {
        let input = RequestInput::new(argument);
        let text = input.text()?;
        match RequestHead::from_text(&text)?.route::<Working::Operation, Meta::Operation>()? {
            CommandLineSocket::Working => {
                let request = RequestText::<Working::Operation>::new(text)
                    .decode_request()?
                    .with_caller(Caller::from_kernel());
                let reply = self.submit::<Working>(self.sockets.working_socket()?, request)?;
                Self::encode_reply(&reply)
            }
            CommandLineSocket::Meta => {
                let request = RequestText::<Meta::Operation>::new(text)
                    .decode_request()?
                    .with_caller(Caller::from_kernel());
                let reply = self.submit::<Meta>(self.sockets.meta_socket()?, request)?;
                Self::encode_reply(&reply)
            }
        }
    }

    fn submit<Frame>(
        &self,
        socket: &Path,
        request: Request<Frame::Operation>,
    ) -> Result<NonEmpty<Frame::Reply>, CommandLineError>
    where
        Frame: ClientFrame,
        Frame::Reply: Debug,
    {
        let mut stream = UnixStream::connect(socket).map_err(CommandLineError::input_output)?;
        // Each invocation opens a fresh connection, writes exactly one request,
        // reads exactly one reply, and then drops the stream. Sequence zero is
        // therefore the sole connector-lane exchange on this connection.
        let exchange = Self::exchange();
        let frame = Frame::request_frame(exchange, request)?;
        self.write_frame(&mut stream, &frame)?;
        let reply = self.read_frame::<Frame>(&mut stream)?;
        Self::reply_payloads(reply.reply_from_frame(exchange)?)
    }

    fn write_frame<Frame>(
        &self,
        stream: &mut UnixStream,
        frame: &Frame,
    ) -> Result<(), CommandLineError>
    where
        Frame: ClientFrame,
    {
        let bytes = frame.encode_client_frame()?;
        stream
            .write_all(&bytes)
            .map_err(CommandLineError::input_output)?;
        stream.flush().map_err(CommandLineError::input_output)
    }

    fn read_frame<Frame>(&self, stream: &mut UnixStream) -> Result<Frame, CommandLineError>
    where
        Frame: ClientFrame,
    {
        let mut prefix = [0_u8; 4];
        stream
            .read_exact(&mut prefix)
            .map_err(CommandLineError::input_output)?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length > self.maximum_frame_bytes {
            return Err(CommandLineError::FrameTooLarge {
                found: length,
                limit: self.maximum_frame_bytes,
            });
        }

        let mut bytes = Vec::with_capacity(4 + length);
        bytes.extend_from_slice(&prefix);
        bytes.resize(4 + length, 0);
        stream
            .read_exact(&mut bytes[4..])
            .map_err(CommandLineError::input_output)?;
        Frame::decode_client_frame(&bytes)
    }

    fn reply_payloads<ReplyPayload>(
        reply: Reply<ReplyPayload>,
    ) -> Result<NonEmpty<ReplyPayload>, CommandLineError>
    where
        ReplyPayload: Debug,
    {
        match reply {
            Reply::Accepted { per_operation, .. } => {
                let mut payloads = Vec::with_capacity(per_operation.len());
                for sub_reply in per_operation {
                    match sub_reply {
                        SubReply::Ok(payload) => payloads.push(payload),
                        other => {
                            return Err(CommandLineError::UnexpectedFrame {
                                expected: "accepted operation reply",
                                got: format!("{other:?}"),
                            });
                        }
                    }
                }
                NonEmpty::try_from_vec(payloads).map_err(|error| CommandLineError::SignalFrame {
                    reason: error.to_string(),
                })
            }
            Reply::Rejected { reason } => Err(CommandLineError::RequestRejected {
                reason: reason.to_string(),
            }),
        }
    }

    fn encode_reply<ReplyPayload>(
        replies: &NonEmpty<ReplyPayload>,
    ) -> Result<String, CommandLineError>
    where
        ReplyPayload: NotaEncode,
    {
        if replies.tail().is_empty() {
            Ok(replies.head().to_nota())
        } else {
            Ok(Delimiter::SquareBracket.wrap(replies.iter().map(ReplyPayload::to_nota)))
        }
    }

    fn exchange() -> ExchangeIdentifier {
        ExchangeIdentifier::new(
            SessionEpoch::new(0),
            ExchangeLane::Connector,
            LaneSequence::first(),
        )
    }
}

#[macro_export]
macro_rules! signal_cli {
    ($name:ident, $working_crate:ident $(,)?) => {
        ::signal_frame::paste::paste! {
            fn main() -> ::std::process::ExitCode {
                ::signal_frame::ClientShape::<
                    $working_crate::Frame,
                    [<meta_ $working_crate>]::Frame,
                >::run_from_environment(::std::stringify!($name))
            }
        }
    };
    (
        $name:ident,
        working: $working_frame:path,
        meta: $meta_frame:path $(,)?
    ) => {
        fn main() -> ::std::process::ExitCode {
            ::signal_frame::ClientShape::<$working_frame, $meta_frame>
                ::run_from_environment(::std::stringify!($name))
        }
    };
    (
        $visibility:vis struct $name:ident {
            working $working:path;
            meta $meta:path;
        }
    ) => {
        $visibility struct $name {
            dispatch: ::signal_frame::CommandLineDispatch<$working, $meta>,
        }

        impl $name {
            pub const fn new() -> Self {
                Self {
                    dispatch: ::signal_frame::CommandLineDispatch::<$working, $meta>::new(),
                }
            }

            pub fn route_head(
                &self,
                head: &str,
            ) -> ::std::result::Result<
                ::signal_frame::CommandLineSocket,
                ::signal_frame::CommandLineRouteError,
            > {
                self.dispatch.route_head(head)
            }

            pub const fn table(&self) -> ::signal_frame::CommandLineRouteTable<'static> {
                self.dispatch.table()
            }
        }

        impl ::std::default::Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}
