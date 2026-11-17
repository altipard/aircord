//! `aircord-core`: everything Aircord does that needs neither a window nor a radio. The stores (history, sessions,
//! playbooks), device templates, the NB bootloader protocol, and the two long runs, a playbook and a firmware update,
//! written against [`link::Link`] so they are tested against a fake device. Ported from the Go packages under `internal/`.

pub mod config;
pub mod device;
pub mod filter;
pub mod history;
pub mod lines;
pub mod link;
pub mod ota;
pub mod otanb;
pub mod playbook;
pub mod scan;
pub mod session;
pub mod slug;
