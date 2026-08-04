//! Handle-first local coding-agent broker built on stable ACP v1.

pub mod acp;
pub mod api;
pub mod error;
pub mod model;
pub mod process;
pub mod providers;
pub mod receipt;
pub mod runtime;

pub use api::Broker;
pub use error::{ControlError, Result};
pub use model::*;
pub use providers::*;
pub use receipt::*;
