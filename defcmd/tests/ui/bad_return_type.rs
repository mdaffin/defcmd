use defcmd::command;

struct NotOutput;

#[command]
fn run() -> NotOutput {
    NotOutput
}

fn main() {}
