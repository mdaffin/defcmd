use defcmd::command;

#[command]
fn copy(#[rest] sources: String) {
    let _ = sources;
}

fn main() {}
