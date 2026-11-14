//! The command tree, built from the registered commands by their full paths.

use std::ffi::OsStr;

use crate::spec::{CommandSpec, Registration};

/// One command and the commands directly below it, sorted by name.
pub(crate) struct Node<'a> {
    /// The command's descriptor; for an implicit group a synthesized one, leaked so it is `'static`
    /// like the rest (one small allocation per group per tree build).
    pub spec: &'static CommandSpec,
    /// The registration to run. `None` for an implicit group, which only prints its help.
    pub reg: Option<&'a Registration>,
    pub children: Vec<Node<'a>>,
}

/// The last path segment: the word that selects `spec` below its parent.
fn name_of(spec: &CommandSpec) -> &'static str {
    spec.path.last().copied().unwrap_or_default()
}

impl<'a> Node<'a> {
    /// The tree under the root, which is the registration with an empty path. `None` if there is
    /// no root. A missing middle node becomes an implicit group (no parameters, no doc, no body).
    /// Of two commands with the same path the first wins (`check()` reports the duplicate).
    pub fn build(regs: &[&'a Registration]) -> Option<Node<'a>> {
        let root = regs.iter().find(|r| r.spec.path.is_empty())?;
        Some(Node::grow(
            Some(root),
            root.spec.path,
            root.spec.version,
            regs,
        ))
    }

    fn grow(
        reg: Option<&'a Registration>,
        path: &'static [&'static str],
        version: Option<&'static str>,
        regs: &[&'a Registration],
    ) -> Node<'a> {
        let spec: &'static CommandSpec = match reg {
            Some(r) => r.spec,
            None => Box::leak(Box::new(CommandSpec {
                path,
                aliases: &[],
                doc: "",
                params: &[],
                version,
            })),
        };
        // The next path word of every command below this one, each with its own registration
        // if there is one (the first of them), else a longer command that implies the group.
        let mut names: Vec<(
            &'static str,
            Option<&'a Registration>,
            &'static [&'static str],
        )> = Vec::new();
        for &r in regs {
            let p = r.spec.path;
            if p.len() <= path.len() || !p.starts_with(path) {
                continue;
            }
            let name = p[path.len()];
            let exact = p.len() == path.len() + 1;
            match names.iter_mut().find(|n| n.0 == name) {
                Some(n) => {
                    if exact && n.1.is_none() {
                        n.1 = Some(r);
                    }
                }
                None => names.push((name, exact.then_some(r), &p[..=path.len()])),
            }
        }
        names.sort_by_key(|n| n.0);
        Node {
            spec,
            reg,
            children: names
                .into_iter()
                .map(|(_, reg, child_path)| Node::grow(reg, child_path, version, regs))
                .collect(),
        }
    }

    /// Every command below this one, parents before their children, siblings sorted by name:
    /// sorted by path.
    pub fn descendants(&self) -> Vec<&'static CommandSpec> {
        let mut all = Vec::new();
        for child in &self.children {
            all.push(child.spec);
            all.extend(child.descendants());
        }
        all
    }

    /// The index in `children` of the child named exactly `word`.
    pub fn child_index(&self, word: &OsStr) -> Option<usize> {
        self.children.iter().position(|c| word == name_of(c.spec))
    }
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

    const fn spec(path: &'static [&'static str], doc: &'static str) -> CommandSpec {
        CommandSpec {
            path,
            aliases: &[],
            doc,
            params: &[],
            version: None,
        }
    }

    static ROOT: CommandSpec = CommandSpec {
        version: Some("1.0"),
        ..spec(&[], "root")
    };
    static GET: CommandSpec = spec(&["get"], "get");
    static CHECK: CommandSpec = spec(&["check"], "check");
    static GET_DEEP: CommandSpec = spec(&["get", "deep"], "deep");
    static ORPHAN: CommandSpec = spec(&["nogroup", "leaf"], "orphan");
    static GET_AGAIN: CommandSpec = spec(&["get"], "duplicate");
    static FAR: CommandSpec = spec(&["a", "b", "c"], "far");

    const fn reg(spec: &'static CommandSpec) -> Registration {
        Registration { spec, run: glue }
    }
    static R_ROOT: Registration = reg(&ROOT);
    static R_GET: Registration = reg(&GET);
    static R_CHECK: Registration = reg(&CHECK);
    static R_DEEP: Registration = reg(&GET_DEEP);
    static R_ORPHAN: Registration = reg(&ORPHAN);
    static R_AGAIN: Registration = reg(&GET_AGAIN);
    static R_FAR: Registration = reg(&FAR);

    fn names(node: &Node) -> Vec<&'static str> {
        node.children.iter().map(|c| name_of(c.spec)).collect()
    }

    #[test]
    fn no_root_means_no_tree() {
        assert!(Node::build(&[&R_GET]).is_none());
        assert!(Node::build(&[]).is_none());
    }

    #[test]
    fn children_are_sorted_by_name_whatever_the_registration_order() {
        let tree = Node::build(&[&R_GET, &R_ROOT, &R_CHECK]).unwrap();
        assert_eq!(tree.spec.doc, "root");
        assert_eq!(names(&tree), ["check", "get"]);
    }

    #[test]
    fn deeper_paths_nest_under_their_parent() {
        let tree = Node::build(&[&R_DEEP, &R_GET, &R_ROOT]).unwrap();
        assert_eq!(names(&tree), ["get"]);
        assert_eq!(names(&tree.children[0]), ["deep"]);
    }

    #[test]
    fn a_missing_middle_node_becomes_a_help_only_group() {
        let tree = Node::build(&[&R_ORPHAN, &R_ROOT]).unwrap();
        assert_eq!(names(&tree), ["nogroup"]);
        let group = &tree.children[0];
        assert!(group.reg.is_none());
        assert_eq!(group.spec.path, ["nogroup"]);
        assert_eq!(group.spec.doc, "");
        assert!(group.spec.params.is_empty());
        assert_eq!(group.spec.version, Some("1.0"));
        assert_eq!(names(group), ["leaf"]);
        assert_eq!(group.children[0].spec.doc, "orphan");
    }

    #[test]
    fn a_declared_node_is_not_a_group_whatever_the_order() {
        let tree = Node::build(&[&R_DEEP, &R_GET, &R_ROOT]).unwrap();
        assert!(tree.children[0].reg.is_some());
        assert_eq!(tree.children[0].spec.doc, "get");
        let tree = Node::build(&[&R_DEEP, &R_ROOT, &R_GET]).unwrap();
        assert_eq!(tree.children[0].spec.doc, "get");
        assert_eq!(tree.children.len(), 1);
    }

    #[test]
    fn groups_nest_and_appear_in_descendants() {
        let tree = Node::build(&[&R_ROOT, &R_FAR]).unwrap();
        let paths: Vec<_> = tree
            .descendants()
            .iter()
            .map(|s| s.path.join(" "))
            .collect();
        assert_eq!(paths, ["a", "a b", "a b c"]);
    }

    #[test]
    fn the_first_of_two_commands_with_one_path_wins() {
        let tree = Node::build(&[&R_ROOT, &R_GET, &R_AGAIN]).unwrap();
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].spec.doc, "get");
    }

    #[test]
    fn descendants_are_flattened_in_path_order() {
        let tree = Node::build(&[&R_GET, &R_DEEP, &R_CHECK, &R_ROOT]).unwrap();
        let paths: Vec<_> = tree
            .descendants()
            .iter()
            .map(|s| s.path.join(" "))
            .collect();
        assert_eq!(paths, ["check", "get", "get deep"]);
        assert!(tree.children[0].descendants().is_empty());
    }

    #[test]
    fn child_lookup_is_an_exact_match() {
        let tree = Node::build(&[&R_ROOT, &R_GET, &R_CHECK]).unwrap();
        assert_eq!(tree.child_index(OsStr::new("check")), Some(0));
        assert_eq!(tree.child_index(OsStr::new("get")), Some(1));
        assert_eq!(tree.child_index(OsStr::new("Get")), None);
        assert_eq!(tree.child_index(OsStr::new("ge")), None);
        assert_eq!(tree.child_index(OsStr::new("")), None);
    }
}
