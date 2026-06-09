//! Process scanning and process→worktree mapping.
//!
//! Best-effort by design (handoff §22.3): a process CWD may be unavailable due
//! to OS permissions, and processes can exit between scan and display. We map
//! what we can and degrade gracefully.

pub mod classifier;
pub mod scanner;
pub mod types;

pub use scanner::{scan, scan_agent_cwds};
pub use types::{ProcessInfo, ProcessLabel, ProcessSnapshot};
