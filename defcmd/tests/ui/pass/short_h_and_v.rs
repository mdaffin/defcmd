use defcmd::command;

/// User `-h` and `-V` replace the generated short forms; the long forms stay.
#[command]
fn main(#[flag(-h)] host: bool, #[flag(-V)] verbose: bool) {
    let _ = (host, verbose);
}
