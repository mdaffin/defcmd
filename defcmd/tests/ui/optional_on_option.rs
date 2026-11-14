use defcmd::command;

#[command]
fn run(#[optional] name: Option<String>) {
    let _ = name;
}

fn main() {}
