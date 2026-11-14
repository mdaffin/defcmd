use defcmd::command;

#[command]
fn run(#[flag] #[default(1)] jobs: Option<usize>) {
    let _ = jobs;
}

fn main() {}
