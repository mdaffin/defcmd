use defcmd::command;

type Maybe<T> = Option<T>;

/// An alias of `Option` is optional only when marked.
#[command]
fn main(
    #[optional] file: Maybe<String>,
    #[flag] #[optional] out: Maybe<String>,
) {
    let _: Option<String> = file;
    let _: Option<String> = out;
}
