//! The live engine: filesystem watching + debounced refresh requests.
//!
//! Filesystem events are *hints* only — they trigger a re-capture, and the Git
//! snapshot remains the source of truth (handoff §13.1). A periodic poll in the
//! event loop backstops any events the watcher misses.

pub mod debouncer;
pub mod fs_watcher;
