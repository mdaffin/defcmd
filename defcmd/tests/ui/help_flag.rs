use defcmd::command;

#[command]
fn run(#[flag] help: bool) {
    let _ = help;
}

fn main() {}
