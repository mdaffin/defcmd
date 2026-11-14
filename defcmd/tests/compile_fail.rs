//! `trybuild` tests of the compile errors `#[command]` raises. Each `ui/*.rs` file must fail to
//! compile with the message in its `.stderr`; `ui/pass/*.rs` must compile.
//!
//! After changing a message, review and refresh with `TRYBUILD=overwrite cargo test --test compile_fail`.

#[test]
fn compile_errors() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass/*.rs");
    t.compile_fail("tests/ui/*.rs");
}
