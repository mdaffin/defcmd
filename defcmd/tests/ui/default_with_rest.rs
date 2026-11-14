use defcmd::command;

#[command]
fn run(#[default(vec![])] #[rest] files: Vec<String>) {
    let _ = files;
}

fn main() {}
