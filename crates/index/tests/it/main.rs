//! Integration tests, one binary so the linked test program (SQLite and every grammar) is built once.

mod common;
mod coverage;
mod golden;
mod incremental;
mod mixed;
mod rust_reexports;
mod synthetic;
mod worktrees;
