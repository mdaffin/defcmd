use defcmd::command;

#[command]
fn run(#[flag] version: bool) {
    let _ = version;
}

fn main() {}
