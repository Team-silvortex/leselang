//! Desktop-only WebView wiring. Portable catalog, gateway and VM execution live
//! in the library; CLI discovery never initializes a WebView.
mod agent;
mod form;
mod model;
mod wire;

pub(super) use agent::launch as launch_agent;
pub(super) use agent::launch_article as launch_agent_article;
pub(super) use agent::launch_board as launch_agent_board;
pub(super) use form::launch as launch_form;
