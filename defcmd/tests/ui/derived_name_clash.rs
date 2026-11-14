use defcmd::command;

#[command]
fn run(#[flag(--dry-run)] a: bool, #[flag] dry_run: bool) {
    let _ = (a, dry_run);
}

fn main() {}
