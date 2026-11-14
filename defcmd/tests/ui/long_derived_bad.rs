use defcmd::command;

#[command]
fn run(#[flag] _verbose: bool) {
    let _ = _verbose;
}

fn main() {}
