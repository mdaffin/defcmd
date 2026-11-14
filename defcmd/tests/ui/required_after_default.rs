use defcmd::command;

#[command]
fn copy(#[default(1)] count: u32, source: String) {
    let _ = (source, count);
}

fn main() {}
