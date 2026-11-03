#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

pub mod edit;
pub mod event;
pub mod execution;
pub mod model;
pub mod policy;
pub mod repository;
pub mod runtime;
pub mod settings;

pub const EVENT_SCHEMA_VERSION: u16 = 1;
