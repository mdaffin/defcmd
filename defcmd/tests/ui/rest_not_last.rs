use defcmd::command;

#[command]
fn copy(#[rest] sources: Vec<String>, target: String) {
    let _ = (sources, target);
}

fn main() {}
