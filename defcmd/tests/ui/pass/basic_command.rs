use defcmd::command;

/// The root.
#[command]
fn main(
    /// A file.
    file: Option<String>,
    #[flag(-v)] verbose: bool,
) {
    let _ = (file, verbose);
}
