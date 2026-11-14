use defcmd::command;

#[command]
fn copy(sources: Vec<String>, target: String) {
    let _ = (sources, target);
}

fn main() {}
