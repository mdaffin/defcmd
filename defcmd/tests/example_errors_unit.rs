//! Runs the unit tests inside `examples/error_handling.rs`.
//!
//! A `test = true` example is built as a test harness only, with no binary for
//! `tests/examples.rs` to run, so the example file is mounted here as a module instead.

#[allow(dead_code)]
#[path = "../examples/error_handling.rs"]
mod error_handling;
