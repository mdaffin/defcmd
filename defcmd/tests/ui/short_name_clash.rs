use defcmd::command;

#[command]
fn run(#[flag(-o)] out: bool, #[flag(-o)] other: bool) {
    let _ = (out, other);
}

fn main() {}
