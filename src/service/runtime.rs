// Runtime-internal mechanisms (not named outside this module).
mod binding;
mod cache;
mod engine;
mod resolved;

// The runtime's cross-crate API (consumed by `job::executor`, `main`, and
// `service::workflow`) plus the extension-author surface (`extension`, and the
// `ctx`/`wit`/`types` items that appear in `Extension` signatures).
pub(crate) mod component;
pub(crate) mod ctx;
pub(crate) mod extension;
pub(crate) mod image;
pub(crate) mod runner;
pub(crate) mod types;
pub(crate) mod wit;
