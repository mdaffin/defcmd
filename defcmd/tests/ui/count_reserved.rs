use defcmd::command;

#[command]
fn run(#[count] verbose: u8) {
    let _ = verbose;
}

fn main() {}
