//! `aircord-core`: everything Aircord does that needs neither a window nor a radio. The stores (history, sessions,
//! playbooks), device templates with typed settings, the NB bootloader protocol, and the long runs (a playbook, a
//! firmware update, a settings transfer), written against [`link::Link`] so they are tested against a fake device.
//! Ported from the Go packages under `internal/`; the settings ([`settings`], [`cfg`], [`transfer`]) are Rust only.

pub mod banner;
pub mod cfg;
pub mod config;
pub mod device;
pub mod filter;
pub mod history;
pub mod lines;
pub mod link;
pub mod ota;
pub mod otanb;
pub mod playbook;
pub mod redact;
pub mod scan;
pub mod session;
pub mod settings;
pub mod slug;
pub mod transfer;
