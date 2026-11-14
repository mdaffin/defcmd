//! The command tree, built from the registered commands by their full paths.

use std::ffi::OsStr;

use crate::spec::{CommandSpec, Registration};

/// One command and the commands directly below it, sorted by name.
pub(crate) struct Node<'a> {
    pub reg: &'a Registration,
    pub children: Vec<Node<'a>>,
}

/// The last path segment: the word that selects `reg` below its parent.
fn name_of(reg: &Registration) -> &'static str {
    reg.spec.path.last().copied().unwrap_or_default()
}

impl<'a> Node<'a> {
    /// The tree under the root, which is the registration with an empty path. `None` if there is
    /// no root. A command whose parent path is not registered is not reachable and is left out;
    /// of two commands with the same path the first wins (`check()` reports the duplicate).
    pub fn build(regs: &[&'a Registration]) -> Option<Node<'a>> {
        let root = regs.iter().find(|r| r.spec.path.is_empty())?;
        Some(Node::grow(root, regs))
    }

    fn grow(reg: &'a Registration, regs: &[&'a Registration]) -> Node<'a> {
        let path = reg.spec.path;
        let mut children: Vec<&'a Registration> = Vec::new();
        for &r in regs {
            let p = r.spec.path;
            if p.len() == path.len() + 1
                && p.starts_with(path)
                && !children.iter().any(|c| name_of(c) == name_of(r))
            {
                children.push(r);
            }
        }
        children.sort_by_key(|c| name_of(c));
        Node {
            reg,
            children: children.into_iter().map(|c| Node::grow(c, regs)).collect(),
        }
    }

    /// Every command below this one, parents before their children, siblings sorted by name:
    /// sorted by path.
    pub fn descendants(&self) -> Vec<&'static CommandSpec> {
        let mut all = Vec::new();
        for child in &self.children {
            all.push(child.reg.spec);
            all.extend(child.descendants());
        }
        all
    }

    /// The index in `children` of the child named exactly `word`.
    pub fn child_index(&self, word: &OsStr) -> Option<usize> {
        self.children.iter().position(|c| word == name_of(c.reg))
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

    static ROOT: CommandSpec = spec(&[], "root");
    static GET: CommandSpec = spec(&["get"], "get");
    static CHECK: CommandSpec = spec(&["check"], "check");
    static GET_DEEP: CommandSpec = spec(&["get", "deep"], "deep");
    static ORPHAN: CommandSpec = spec(&["nogroup", "leaf"], "orphan");
    static GET_AGAIN: CommandSpec = spec(&["get"], "duplicate");

    const fn reg(spec: &'static CommandSpec) -> Registration {
        Registration { spec, run: glue }
    }
    static R_ROOT: Registration = reg(&ROOT);
    static R_GET: Registration = reg(&GET);
    static R_CHECK: Registration = reg(&CHECK);
    static R_DEEP: Registration = reg(&GET_DEEP);
    static R_ORPHAN: Registration = reg(&ORPHAN);
    static R_AGAIN: Registration = reg(&GET_AGAIN);

    fn names(node: &Node) -> Vec<&'static str> {
        node.children.iter().map(|c| name_of(c.reg)).collect()
    }

    #[test]
    fn no_root_means_no_tree() {
        assert!(Node::build(&[&R_GET]).is_none());
        assert!(Node::build(&[]).is_none());
    }

    #[test]
    fn children_are_sorted_by_name_whatever_the_registration_order() {
        let tree = Node::build(&[&R_GET, &R_ROOT, &R_CHECK]).unwrap();
        assert_eq!(tree.reg.spec.doc, "root");
        assert_eq!(names(&tree), ["check", "get"]);
    }

    #[test]
    fn deeper_paths_nest_under_their_parent() {
        let tree = Node::build(&[&R_DEEP, &R_GET, &R_ROOT]).unwrap();
        assert_eq!(names(&tree), ["get"]);
        assert_eq!(names(&tree.children[0]), ["deep"]);
    }

    #[test]
    fn a_command_without_a_registered_parent_is_left_out() {
        let tree = Node::build(&[&R_ORPHAN, &R_ROOT]).unwrap();
        assert!(tree.children.is_empty());
    }

    #[test]
    fn the_first_of_two_commands_with_one_path_wins() {
        let tree = Node::build(&[&R_ROOT, &R_GET, &R_AGAIN]).unwrap();
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].reg.spec.doc, "get");
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
