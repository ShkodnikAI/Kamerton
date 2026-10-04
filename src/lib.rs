//! Kamerton — personal n-of-1 tuning system.
//!
//! The deterministic engine lives in this crate: storage, arms, bandit,
//! blind context, honesty valve and reporting. The language model only
//! formulates and translates; it never decides whether a protocol works
//! (AGENTS.md invariant 1). Modules grow wave by wave: `store`, `arms`,
//! `bandit`, `context`, `honesty`, `report`, `telegram`.

pub mod arms;
pub mod bandit;
pub mod context;
pub mod honesty;
pub mod report;
pub mod store;
pub mod telegram;
