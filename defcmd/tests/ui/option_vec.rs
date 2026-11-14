use defcmd::command;

#[command]
fn run(#[flag] names: Option<Vec<String>>) {
    let _ = names;
}

fn main() {}
