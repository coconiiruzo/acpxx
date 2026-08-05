//! Handle-first local coding-agent broker built on stable ACP v1.

pub mod acp;
pub mod api;
#[cfg(unix)]
pub mod benchmark;
pub mod compatibility;
pub mod config;
pub mod doctor;
pub mod error;
#[cfg(unix)]
pub mod ipc;
pub mod model;
pub mod process;
pub mod providers;
pub mod receipt;
pub mod runtime;
pub mod security;
pub mod storage;

pub use api::Broker;
pub use compatibility::*;
pub use error::{AdmissionError, ControlError, Result};
pub use model::*;
pub use providers::*;
pub use receipt::*;
