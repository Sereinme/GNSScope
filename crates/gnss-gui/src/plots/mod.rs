//! View-only plot drawing modules composed by the application shell.
//!
//! Modules here render display data supplied by `app.rs`; callbacks and state
//! ownership stay with the application.

pub(crate) mod cn0;
pub(crate) mod enu;
pub(crate) mod frame;
pub(crate) mod pvt;
pub(crate) mod sky;
pub(crate) mod timeline;
