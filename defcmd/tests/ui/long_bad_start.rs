use defcmd::command;

#[command]
fn run(#[flag(--1x)] verbose: bool) {
    let _ = verbose;
}

fn main() {}
