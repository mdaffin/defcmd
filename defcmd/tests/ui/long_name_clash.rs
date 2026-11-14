use defcmd::command;

#[command]
fn run(#[flag(--out)] a: bool, #[flag(--out)] b: bool) {
    let _ = (a, b);
}

fn main() {}
