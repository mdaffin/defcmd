use defcmd::command;

#[command]
fn run(#[flag] #[rest] files: Vec<String>) {
    let _ = files;
}

fn main() {}
