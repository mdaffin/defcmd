use defcmd::command;

type Maybe<T> = Option<T>;

// Without `#[optional]` the alias is a required positional, so it cannot follow an optional one.
#[command]
fn run(first: Option<String>, second: Maybe<String>) {
    let _ = (first, second);
}

fn main() {}
