//! Color and style constants. A no-color fallback mode lands with the config
//! subsystem (handoff §15).

use ratatui::style::Color;

/// Primary accent (selected tab, brand mark).
pub const ACCENT: Color = Color::Cyan;
/// De-emphasized text (hints, secondary labels).
pub const DIM: Color = Color::DarkGray;
/// Dirty / modified state.
pub const DIRTY: Color = Color::Yellow;
/// Collision / conflict state.
pub const COLLISION: Color = Color::Red;
/// Clean / synced state.
pub const CLEAN: Color = Color::Green;
