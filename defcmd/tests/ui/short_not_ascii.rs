use defcmd::command;

#[command]
fn run(#[flag(-é)] verbose: bool) {
    let _ = verbose;
}

fn main() {}
