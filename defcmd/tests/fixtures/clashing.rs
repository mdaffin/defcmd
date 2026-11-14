//! A deliberately invalid command tree: start-up validation must panic (see `tests/examples.rs`).

use defcmd::command;

/// The root.
#[command]
fn main() {}

/// One `get`.
#[command("get")]
fn first() {}

/// Another `get`.
#[command("get")]
fn second() {}

/// Its alias is the name of the group implied by `remote add`.
#[command(alias = remote)]
fn push() {}

/// Implies the group `remote`.
#[command("remote add")]
fn remote_add() {}
