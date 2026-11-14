use defcmd::command;

#[command]
fn run(#[flag] color: Option<bool>, #[flag] no_color: bool) {
    let _ = (color, no_color);
}

fn main() {}
