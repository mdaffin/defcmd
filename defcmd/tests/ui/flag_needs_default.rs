use defcmd::command;

#[command]
fn run(#[flag] jobs: usize) {
    let _ = jobs;
}

fn main() {}
