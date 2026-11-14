//! Validation of the command tree. Only the plumbing and the duplicate-path
//! check so far; the other checks come with M12.

use crate::spec::Registration;

/// Every problem found among `regs`, in a stable order.
pub(crate) fn problems(regs: &[&Registration]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen: Vec<&[&str]> = Vec::new();
    for reg in regs {
        let path = reg.spec.path;
        if seen.contains(&path) {
            continue;
        }
        seen.push(path);
        let count = regs.iter().filter(|r| r.spec.path == path).count();
        if count > 1 {
            let shown = if path.is_empty() {
                "the root command".to_string()
            } else {
                format!("command '{}'", path.join(" "))
            };
            problems.push(format!("{shown} is registered {count} times"));
        }
    }
    problems
}

/// Panics with every problem in `regs`, or returns if there is none.
pub(crate) fn check_registrations(regs: &[&Registration]) {
    let problems = problems(regs);
    if !problems.is_empty() {
        let list: String = problems.iter().map(|p| format!("\n  - {p}")).collect();
        panic!("invalid command tree:{list}");
    }
}

/// Validates the whole command tree and panics with every problem found. Meant to be called from
/// one `#[test]`: the test binary links the commands in, so the registry is complete there.
pub fn check() {
    let regs: Vec<&Registration> = inventory::iter::<Registration>.into_iter().collect();
    check_registrations(&regs);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Parsed, UsageError};
    use crate::spec::CommandSpec;
    use std::io::Write;
    use std::process::ExitCode;

    fn glue(_: &Parsed, _: &mut dyn Write) -> Result<ExitCode, UsageError> {
        Ok(ExitCode::SUCCESS)
    }

    const fn reg(spec: &'static CommandSpec) -> Registration {
        Registration { spec, run: glue }
    }
    const fn spec(path: &'static [&'static str]) -> CommandSpec {
        CommandSpec {
            path,
            aliases: &[],
            doc: "",
            params: &[],
            version: None,
        }
    }

    static ROOT: CommandSpec = spec(&[]);
    static GET: CommandSpec = spec(&["get"]);
    static HAS: CommandSpec = spec(&["has"]);
    static R_ROOT: Registration = reg(&ROOT);
    static R_ROOT2: Registration = reg(&ROOT);
    static R_GET: Registration = reg(&GET);
    static R_GET2: Registration = reg(&GET);
    static R_GET3: Registration = reg(&GET);
    static R_HAS: Registration = reg(&HAS);
    static R_HAS2: Registration = reg(&HAS);

    #[test]
    fn a_clean_tree_has_no_problems() {
        assert!(problems(&[&R_ROOT, &R_GET, &R_HAS]).is_empty());
        check_registrations(&[&R_ROOT, &R_GET, &R_HAS]);
        check_registrations(&[]);
    }

    #[test]
    fn duplicate_paths_are_all_reported_once_each() {
        let found = problems(&[&R_GET, &R_ROOT, &R_GET2, &R_HAS, &R_GET3, &R_ROOT2, &R_HAS2]);
        assert_eq!(
            found,
            [
                "command 'get' is registered 3 times",
                "the root command is registered 2 times",
                "command 'has' is registered 2 times",
            ]
        );
    }

    #[test]
    #[should_panic(expected = "invalid command tree:\n  - command 'get' is registered 2 times")]
    fn check_panics_with_the_problems() {
        check_registrations(&[&R_ROOT, &R_GET, &R_GET2]);
    }

    // The registry of this test binary: the commands `run.rs` and `lib.rs` tests register.
    #[test]
    fn the_global_registry_is_valid() {
        check();
    }
}
