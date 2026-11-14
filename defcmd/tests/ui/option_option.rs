use defcmd::command;

#[command]
fn run(name: Option<Option<String>>) {
    let _ = name;
}

fn main() {}
