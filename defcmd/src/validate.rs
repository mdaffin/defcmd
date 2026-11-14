//! Validation of the command tree: clashes between different commands, which
//! only show up at run time because the registry is built at link time.

use crate::spec::Registration;

/// `command 'a b'` or `the root command`.
fn describe_path(path: &[&str]) -> String {
    if path.is_empty() {
        "the root command".to_string()
    } else {
        format!("command '{}'", path.join(" "))
    }
}

/// `a::f (src/a.rs:3), b::g (src/b.rs:9)`.
fn locations(regs: &[&&Registration]) -> String {
    regs.iter()
        .map(|r| r.location.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// One node among the children of a parent: a registered command or an implicit group.
struct Entity<'a> {
    parent: &'a [&'a str],
    /// The name and the aliases.
    names: Vec<&'a str>,
    /// What the problem message calls it.
    shown: String,
}

/// Every problem found among `regs`, in a stable order: duplicate paths, then more than one root,
/// then names that clash among siblings.
pub(crate) fn problems(regs: &[&Registration]) -> Vec<String> {
    let mut problems = Vec::new();

    // 1. Two commands with the same full path (the root is check 3).
    let mut paths: Vec<&[&str]> = Vec::new();
    for reg in regs {
        let path = reg.spec.path;
        if path.is_empty() || paths.contains(&path) {
            continue;
        }
        paths.push(path);
        let same: Vec<&&Registration> = regs.iter().filter(|r| r.spec.path == path).collect();
        if same.len() > 1 {
            problems.push(format!(
                "{} is registered {} times: {}",
                describe_path(path),
                same.len(),
                locations(&same)
            ));
        }
    }

    // 3. More than one root command.
    let roots: Vec<&&Registration> = regs.iter().filter(|r| r.spec.path.is_empty()).collect();
    if roots.len() > 1 {
        problems.push(format!(
            "more than one root command ({}): {}",
            roots.len(),
            locations(&roots)
        ));
    }

    // 2. A name or alias equal to a sibling's name, alias or implicit group name.
    let mut entities: Vec<Entity> = Vec::new();
    let mut registered: Vec<&[&str]> = Vec::new();
    for reg in regs {
        let path = reg.spec.path;
        if path.is_empty() || registered.contains(&path) {
            continue;
        }
        registered.push(path);
        let (name, parent) = path.split_last().expect("path is not empty");
        let mut names = vec![*name];
        names.extend(reg.spec.aliases.iter().copied());
        entities.push(Entity {
            parent,
            names,
            shown: format!("{} ({})", describe_path(path), reg.location),
        });
    }
    let mut groups: Vec<&[&str]> = Vec::new();
    for reg in regs {
        for len in 1..reg.spec.path.len() {
            let prefix = &reg.spec.path[..len];
            if registered.contains(&prefix) || groups.contains(&prefix) {
                continue;
            }
            groups.push(prefix);
            let (name, parent) = prefix.split_last().expect("prefix is not empty");
            entities.push(Entity {
                parent,
                names: vec![*name],
                shown: format!("group '{}' (implied by {})", prefix.join(" "), reg.location),
            });
        }
    }
    for (i, a) in entities.iter().enumerate() {
        for b in &entities[i + 1..] {
            if a.parent != b.parent {
                continue;
            }
            for name in &a.names {
                if b.names.contains(name) {
                    let at = if a.parent.is_empty() {
                        String::new()
                    } else {
                        format!(" under '{}'", a.parent.join(" "))
                    };
                    problems.push(format!(
                        "the name '{name}'{at} is used by both {} and {}",
                        a.shown, b.shown
                    ));
                }
            }
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
    use crate::spec::{CommandSpec, Location};
    use std::io::Write;
    use std::process::ExitCode;

    fn glue(_: &Parsed, _: &mut dyn Write) -> Result<ExitCode, UsageError> {
        Ok(ExitCode::SUCCESS)
    }

    /// A registration whose location is `m::<function> (f.rs:<line>)`.
    const fn reg(spec: &'static CommandSpec, function: &'static str, line: u32) -> Registration {
        Registration {
            spec,
            location: Location {
                module: "m",
                function,
                file: "f.rs",
                line,
            },
            run: glue,
        }
    }
    const fn spec(path: &'static [&'static str], aliases: &'static [&'static str]) -> CommandSpec {
        CommandSpec {
            path,
            aliases,
            doc: "",
            params: &[],
            version: None,
        }
    }

    static ROOT: CommandSpec = spec(&[], &[]);
    static GET: CommandSpec = spec(&["get"], &[]);
    static HAS: CommandSpec = spec(&["has"], &[]);
    static R_ROOT: Registration = reg(&ROOT, "main", 1);
    static R_ROOT2: Registration = reg(&ROOT, "main2", 2);
    static R_GET: Registration = reg(&GET, "get", 3);
    static R_GET2: Registration = reg(&GET, "get2", 4);
    static R_GET3: Registration = reg(&GET, "get3", 5);
    static R_HAS: Registration = reg(&HAS, "has", 6);
    static R_HAS2: Registration = reg(&HAS, "has2", 7);

    #[test]
    fn a_clean_tree_has_no_problems() {
        assert!(problems(&[&R_ROOT, &R_GET, &R_HAS]).is_empty());
        check_registrations(&[&R_ROOT, &R_GET, &R_HAS]);
        check_registrations(&[]);
    }

    #[test]
    fn duplicate_paths_name_every_function() {
        assert_eq!(
            problems(&[&R_ROOT, &R_GET, &R_GET2, &R_GET3]),
            [
                "command 'get' is registered 3 times: m::get (f.rs:3), m::get2 (f.rs:4), m::get3 (f.rs:5)"
            ]
        );
    }

    #[test]
    fn more_than_one_root_is_reported() {
        assert_eq!(
            problems(&[&R_ROOT, &R_ROOT2, &R_GET]),
            ["more than one root command (2): m::main (f.rs:1), m::main2 (f.rs:2)"]
        );
    }

    static REMOTE_ADD: CommandSpec = spec(&["remote", "add"], &[]);
    static REMOTE_RM: CommandSpec = spec(&["remote", "rm"], &[]);
    static PUSH: CommandSpec = spec(&["push"], &["remote"]);
    static FETCH: CommandSpec = spec(&["fetch"], &["get"]);
    static REMOTE_REMOVE: CommandSpec = spec(&["remote", "remove"], &["rm"]);
    static R_REMOTE_ADD: Registration = reg(&REMOTE_ADD, "remote_add", 10);
    static R_REMOTE_RM: Registration = reg(&REMOTE_RM, "remote_rm", 11);
    static R_REMOTE_REMOVE: Registration = reg(&REMOTE_REMOVE, "remote_remove", 12);
    static R_PUSH: Registration = reg(&PUSH, "push", 13);
    static R_FETCH: Registration = reg(&FETCH, "fetch", 14);

    #[test]
    fn an_alias_clashing_with_an_implicit_group_is_reported() {
        assert_eq!(
            problems(&[&R_ROOT, &R_PUSH, &R_REMOTE_ADD]),
            [
                "the name 'remote' is used by both command 'push' (m::push (f.rs:13)) and group 'remote' (implied by m::remote_add (f.rs:10))"
            ]
        );
    }

    #[test]
    fn an_alias_clashing_with_a_command_name_or_alias_is_reported() {
        assert_eq!(
            problems(&[&R_GET, &R_FETCH]),
            [
                "the name 'get' is used by both command 'get' (m::get (f.rs:3)) and command 'fetch' (m::fetch (f.rs:14))"
            ]
        );
        assert_eq!(
            problems(&[&R_REMOTE_RM, &R_REMOTE_REMOVE]),
            [
                "the name 'rm' under 'remote' is used by both command 'remote rm' (m::remote_rm (f.rs:11)) and command 'remote remove' (m::remote_remove (f.rs:12))"
            ]
        );
    }

    #[test]
    fn the_same_name_at_different_levels_is_fine() {
        static GET_GET: CommandSpec = spec(&["get", "get"], &[]);
        static R_GET_GET: Registration = reg(&GET_GET, "get_get", 20);
        assert!(problems(&[&R_ROOT, &R_GET, &R_GET_GET]).is_empty());
    }

    #[test]
    fn several_problems_are_all_reported() {
        let found = problems(&[
            &R_ROOT,
            &R_GET,
            &R_ROOT2,
            &R_GET2,
            &R_HAS,
            &R_FETCH,
            &R_PUSH,
            &R_REMOTE_ADD,
        ]);
        assert_eq!(found.len(), 4, "{found:#?}");
        assert!(found[0].starts_with("command 'get' is registered 2 times"));
        assert!(found[1].starts_with("more than one root command (2)"));
        assert!(found[2].starts_with("the name 'get' is used by both"));
        assert!(found[3].starts_with("the name 'remote' is used by both"));
    }

    #[test]
    #[should_panic(
        expected = "invalid command tree:\n  - command 'get' is registered 2 times: m::get"
    )]
    fn check_panics_with_the_problems() {
        check_registrations(&[&R_ROOT, &R_GET, &R_GET2]);
    }

    // The registry of this test binary: the commands `run.rs` and `lib.rs` tests register.
    #[test]
    fn the_global_registry_is_valid() {
        check();
    }

    #[test]
    fn duplicate_paths_are_all_reported_once_each() {
        let found = problems(&[&R_GET, &R_ROOT, &R_GET2, &R_HAS, &R_GET3, &R_HAS2]);
        assert_eq!(found.len(), 2);
    }
}
