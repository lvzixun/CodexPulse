//! UI-independent, local usage accounting for CodexPulse.
pub mod collectors;
pub mod domain;
pub mod ledger;
pub mod news;
pub mod pricing;
pub mod quota;
pub mod storage;

pub const PARSER_VERSION: u32 = 3;
