use defcmd::command;

#[command]
fn copy(target: Option<String>, source: String) {
    let _ = (source, target);
}

fn main() {}
