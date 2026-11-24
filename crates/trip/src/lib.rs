//! The parts of `trip` other programs build on: the local model, the agent and its jobs,
//! and the logo (its geometry and intro, shared with the desktop's screens).
//! `tripppin-ui` (the orb) uses the same agent as the terminal.

pub mod agent;
pub mod jobs;
pub mod logo;
pub mod markdown;
pub mod model;
pub mod providers;
pub mod session;
pub mod term;
pub mod tui;
