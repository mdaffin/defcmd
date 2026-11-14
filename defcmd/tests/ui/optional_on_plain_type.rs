use defcmd::command;

#[command]
fn run(#[optional] name: String) {
    let _ = name;
}

fn main() {}
