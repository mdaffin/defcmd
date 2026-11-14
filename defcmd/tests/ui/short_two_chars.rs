use defcmd::command;

#[command]
fn run(#[flag(-ab)] verbose: bool) {
    let _ = verbose;
}

fn main() {}
