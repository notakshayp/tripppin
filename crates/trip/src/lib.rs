//! The parts of `trip` other programs build on: the local model, the agent and its jobs.
//! `tripppin-ui` (the orb) uses the same agent as the terminal.

pub mod agent;
pub mod jobs;
pub mod model;
pub mod models;
pub mod permissions;
pub mod providers;
pub mod session;
pub mod term;
