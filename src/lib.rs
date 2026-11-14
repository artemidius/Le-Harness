#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

pub mod context;
pub mod credentials;
pub mod edit;
pub mod eval;
pub mod event;
pub mod execution;
pub mod fim;
pub mod local;
pub mod mcp;
pub mod model;
pub mod policy;
pub mod repository;
pub mod router;
pub mod runtime;
pub mod session;
pub mod settings;
pub mod tools;
pub mod tui;

pub const EVENT_SCHEMA_VERSION: u16 = 1;
