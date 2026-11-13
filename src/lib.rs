#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

pub mod event;
pub mod model;
pub mod settings;

pub const EVENT_SCHEMA_VERSION: u16 = 1;
