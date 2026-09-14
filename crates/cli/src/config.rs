//! Re-exports the config types (v0.2.12: extracted to `sherwood-config` so
//! `sherwood-server` can depend on them too, without a cycle). Kept as a
//! module here so every existing `crate::config::...` path in this crate
//! keeps working unchanged. Two conversions that need `sherwood-server`/
//! `sherwood-wallets` types couldn't move with the rest — see
//! `crate::config_ext` and `sherwood-config`'s `Cargo.toml` for why.

pub use sherwood_config::*;
