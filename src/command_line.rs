use std::marker::PhantomData;

use thiserror::Error;

/// Request-payload enums expose their NOTA record heads through this
/// trait. `signal_channel!` implements it for the generated operation
/// enum so command-line dispatch can route before full decode.
pub trait SignalOperationHeads {
    const HEADS: &'static [&'static str];

    fn contains_head(head: &str) -> bool {
        Self::HEADS.contains(&head)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLineSocket {
    Working,
    Owner,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandLineRouteError {
    #[error("unknown request head: {head}")]
    UnknownRequestHead { head: String },

    #[error("request head appears in both working and owner contracts: {head}")]
    AmbiguousRequestHead { head: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandLineRouteTable<'head> {
    working_heads: &'head [&'head str],
    owner_heads: &'head [&'head str],
}

impl<'head> CommandLineRouteTable<'head> {
    pub const fn new(working_heads: &'head [&'head str], owner_heads: &'head [&'head str]) -> Self {
        Self {
            working_heads,
            owner_heads,
        }
    }

    pub fn route_head(&self, head: &str) -> Result<CommandLineSocket, CommandLineRouteError> {
        match (
            self.working_heads.contains(&head),
            self.owner_heads.contains(&head),
        ) {
            (true, false) => Ok(CommandLineSocket::Working),
            (false, true) => Ok(CommandLineSocket::Owner),
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

    pub const fn owner_heads(&self) -> &'head [&'head str] {
        self.owner_heads
    }
}

pub struct CommandLineDispatch<Working, Owner> {
    table: CommandLineRouteTable<'static>,
    marker: PhantomData<fn() -> (Working, Owner)>,
}

impl<Working, Owner> CommandLineDispatch<Working, Owner>
where
    Working: SignalOperationHeads,
    Owner: SignalOperationHeads,
{
    pub const fn new() -> Self {
        Self {
            table: CommandLineRouteTable::new(Working::HEADS, Owner::HEADS),
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

impl<Working, Owner> Default for CommandLineDispatch<Working, Owner>
where
    Working: SignalOperationHeads,
    Owner: SignalOperationHeads,
{
    fn default() -> Self {
        Self::new()
    }
}

#[macro_export]
macro_rules! signal_cli {
    (
        $visibility:vis struct $name:ident {
            working $working:path;
            owner $owner:path;
        }
    ) => {
        $visibility struct $name {
            dispatch: ::signal_frame::CommandLineDispatch<$working, $owner>,
        }

        impl $name {
            pub const fn new() -> Self {
                Self {
                    dispatch: ::signal_frame::CommandLineDispatch::<$working, $owner>::new(),
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
