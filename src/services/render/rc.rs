//! Rc Rendering
//!
//! Shell rc text rendering with per-tool marker sections.

use crate::model::state::artifact::BlameSet;
use crate::model::state::rc::EnvEntry;
use crate::model::state::rc::InitEntry;
use crate::model::state::rc::ProfileEntry;
use crate::model::state::rc::RcData;

use super::shell_escape::escape_argv;

/// Fallback owner when a blame entry is absent.
const UNKNOWN_TOOL: &str = "unknown";

/// Renders rc data to shell text with per-tool marker sections.
///
/// # Arguments
///
/// * `data` - the merged per-shell entries.
/// * `blame` - the per-entry winner attribution.
///
/// # Returns
///
/// Shell text. Empty data yields the empty string; empty sections yield zero lines.
///
/// # Examples
/// ```rust
/// use confit::model::state::artifact::BlameSet;
/// use confit::model::state::rc::EnvEntry;
/// use confit::model::state::rc::RcData;
/// use confit::services::render::rc::render_rc;
///
/// let data = RcData {
///     profile: Vec::new(),
///     env: vec![EnvEntry { name: "A".into(), value: "1".into(), when: None }],
///     aliases: [("cat".to_string(), "bat".to_string())].into_iter().collect(),
///     init: Vec::new(),
/// };
/// let mut blame = BlameSet::default();
/// blame.env.push("zoxide".into());
/// blame.aliases.insert("cat".into(), "bat".into());
/// let rendered = render_rc(&data, &blame);
/// assert!(rendered.contains("# >>> confit:zoxide"));
/// assert!(rendered.contains("export A=1"));
/// ```
pub fn render_rc(data: &RcData, blame: &BlameSet) -> String {
    let mut env_source: Vec<(String, String)> = Vec::new();
    for (index, entry) in data.profile.iter().enumerate() {
        let tool = blame
            .profile
            .get(index)
            .cloned()
            .unwrap_or_else(|| UNKNOWN_TOOL.to_string());
        env_source.push((tool, render_profile_entry(entry)));
    }
    for (index, entry) in data.env.iter().enumerate() {
        let tool = blame
            .env
            .get(index)
            .cloned()
            .unwrap_or_else(|| UNKNOWN_TOOL.to_string());
        env_source.push((tool, render_env_entry(entry)));
    }

    let mut alias_source: Vec<(String, String)> = Vec::new();
    for (name, value) in &data.aliases {
        let tool = blame
            .aliases
            .get(name)
            .cloned()
            .unwrap_or_else(|| UNKNOWN_TOOL.to_string());
        alias_source.push((tool, render_alias_line(name, value)));
    }

    let mut init_source: Vec<(String, String)> = Vec::new();
    for (index, entry) in data.init.iter().enumerate() {
        let tool = blame
            .init
            .get(index)
            .cloned()
            .unwrap_or_else(|| UNKNOWN_TOOL.to_string());
        init_source.push((tool, render_init_entry(entry)));
    }

    let mut blocks: Vec<String> = Vec::new();
    if !env_source.is_empty() {
        blocks.push(render_groups(&group_ordered(env_source)));
    }
    if !alias_source.is_empty() {
        blocks.push(render_groups(&group_ordered(alias_source)));
    }
    if !init_source.is_empty() {
        blocks.push(render_groups(&group_ordered(init_source)));
    }
    if blocks.is_empty() {
        return String::new();
    }
    let mut out = blocks.join("\n\n");
    out.push('\n');
    out
}

/// Renders one env entry as an export line.
///
/// # Arguments
///
/// * `entry` - the env entry.
///
/// # Returns
///
/// Shell export line for the entry.
fn render_env_entry(entry: &EnvEntry) -> String {
    let value = escape_argv(std::slice::from_ref(&entry.value));
    format!("export {}={value}", entry.name)
}

/// Renders one profile entry as an export line.
///
/// # Arguments
///
/// * `entry` - the profile entry.
///
/// # Returns
///
/// Shell export line for the entry.
fn render_profile_entry(entry: &ProfileEntry) -> String {
    let value = escape_argv(std::slice::from_ref(&entry.value));
    match entry.op {
        crate::model::state::rc::PathOp::Prepend => {
            format!("export {}={value}:\"${{{}}}\"", entry.name, entry.name)
        }
        crate::model::state::rc::PathOp::Append => {
            format!("export {}=\"${{{}}}\":{value}", entry.name, entry.name)
        }
    }
}

/// Renders one alias as an alias line.
///
/// # Arguments
///
/// * `name` - the alias name.
/// * `value` - the alias value.
///
/// # Returns
///
/// Shell alias line.
fn render_alias_line(name: &str, value: &str) -> String {
    let quoted = escape_argv(&[value.to_string()]);
    format!("alias {name}={quoted}")
}

/// Renders one init entry as shell text.
///
/// # Arguments
///
/// * `entry` - the init entry.
///
/// # Returns
///
/// Shell line for the entry.
fn render_init_entry(entry: &InitEntry) -> String {
    match entry {
        InitEntry::Eval { argv } => format!("eval \"$({})\"", escape_argv(argv)),
        InitEntry::Cmd { argv } => escape_argv(argv),
        InitEntry::Source { path } => format!("source {}", escape_argv(std::slice::from_ref(path))),
    }
}

/// Groups tool lines by winner tool.
///
/// # Arguments
///
/// * `pairs` - tool plus line pairs in render order.
///
/// # Returns
///
/// One line list per tool in first-seen tool order.
fn group_ordered(pairs: Vec<(String, String)>) -> Vec<(String, Vec<String>)> {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for (tool, line) in pairs {
        match groups.iter_mut().find(|(name, _)| *name == tool) {
            Some((_, lines)) => lines.push(line),
            None => groups.push((tool, vec![line])),
        }
    }
    groups
}

/// Renders grouped tool sections.
///
/// # Arguments
///
/// * `groups` - tool plus line lists.
///
/// # Returns
///
/// Shell text with marker sections.
fn render_groups(groups: &[(String, Vec<String>)]) -> String {
    groups
        .iter()
        .map(|(tool, lines)| {
            let mut section = format!("# >>> confit:{tool}");
            for line in lines {
                section.push('\n');
                section.push_str(line);
            }
            section.push_str("\n# <<< confit");
            section
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn env(name: &str, value: &str) -> EnvEntry {
        EnvEntry {
            name: name.into(),
            value: value.into(),
            when: None,
        }
    }

    fn profile(value: &str, op: crate::model::state::rc::PathOp) -> ProfileEntry {
        ProfileEntry {
            name: "PATH".into(),
            value: value.into(),
            op,
            when: None,
        }
    }

    #[test]
    fn empty_data_renders_empty_string() {
        assert_eq!(render_rc(&RcData::default(), &BlameSet::default()), "");
    }

    #[test]
    fn env_groups_by_tool_with_markers() {
        let data = RcData {
            profile: Vec::new(),
            env: vec![env("A", "1"), env("B", "x y")],
            aliases: BTreeMap::new(),
            init: Vec::new(),
        };
        let blame = BlameSet {
            env: vec!["first".into(), "second".into()],
            ..BlameSet::default()
        };
        let rendered = render_rc(&data, &blame);
        assert_eq!(
            rendered,
            "# >>> confit:first\nexport A=1\n# <<< confit\n\
             # >>> confit:second\nexport B='x y'\n# <<< confit\n"
        );
    }

    #[test]
    fn three_blocks_join_with_blank_lines() {
        let data = RcData {
            profile: Vec::new(),
            env: vec![env("A", "1")],
            aliases: [("cat".to_string(), "bat".to_string())]
                .into_iter()
                .collect(),
            init: vec![InitEntry::Cmd {
                argv: vec!["task".into(), "--completion".into()],
            }],
        };
        let blame = BlameSet {
            env: vec!["e".into()],
            aliases: [("cat".to_string(), "e".to_string())].into_iter().collect(),
            init: vec!["e".into()],
            ..BlameSet::default()
        };
        let rendered = render_rc(&data, &blame);
        assert_eq!(
            rendered,
            "# >>> confit:e\nexport A=1\n# <<< confit\n\
             \n\
             # >>> confit:e\nalias cat=bat\n# <<< confit\n\
             \n\
             # >>> confit:e\ntask --completion\n# <<< confit\n"
        );
    }

    #[test]
    fn profile_prepends_before_env_in_tool_section() {
        let data = RcData {
            profile: vec![profile("/a", crate::model::state::rc::PathOp::Prepend)],
            env: vec![env("A", "1")],
            aliases: BTreeMap::new(),
            init: Vec::new(),
        };
        let blame = BlameSet {
            profile: vec!["tool".into()],
            env: vec!["tool".into()],
            ..BlameSet::default()
        };
        let rendered = render_rc(&data, &blame);
        assert_eq!(
            rendered,
            "# >>> confit:tool\nexport PATH=/a:\"${PATH}\"\nexport A=1\n# <<< confit\n"
        );
    }

    #[test]
    fn profile_append_renders_value_last() {
        let data = RcData {
            profile: vec![profile("/z", crate::model::state::rc::PathOp::Append)],
            env: Vec::new(),
            aliases: BTreeMap::new(),
            init: Vec::new(),
        };
        let blame = BlameSet {
            profile: vec!["tool".into()],
            ..BlameSet::default()
        };
        assert_eq!(
            render_rc(&data, &blame),
            "# >>> confit:tool\nexport PATH=\"${PATH}\":/z\n# <<< confit\n"
        );
    }

    #[test]
    fn eval_wraps_escaped_argv() {
        let data = RcData {
            profile: Vec::new(),
            env: Vec::new(),
            aliases: BTreeMap::new(),
            init: vec![
                InitEntry::Eval {
                    argv: vec!["zoxide".into(), "init".into(), "bash".into()],
                },
                InitEntry::Eval {
                    argv: vec!["echo".into(), "a b".into()],
                },
            ],
        };
        let blame = BlameSet {
            init: vec!["tool".into(), "tool".into()],
            ..BlameSet::default()
        };
        assert_eq!(
            render_rc(&data, &blame),
            "# >>> confit:tool\neval \"$(zoxide init bash)\"\neval \"$(echo 'a b')\"\n# <<< confit\n"
        );
    }

    #[test]
    fn source_renders_plain_path() {
        let data = RcData {
            profile: Vec::new(),
            env: Vec::new(),
            aliases: BTreeMap::new(),
            init: vec![InitEntry::Source {
                path: "~/.cargo/env".into(),
            }],
        };
        let blame = BlameSet {
            init: vec!["tool".into()],
            ..BlameSet::default()
        };
        assert_eq!(
            render_rc(&data, &blame),
            "# >>> confit:tool\nsource '~/.cargo/env'\n# <<< confit\n"
        );
    }

    #[test]
    fn missing_blame_falls_back_to_unknown() {
        let data = RcData {
            profile: Vec::new(),
            env: vec![env("A", "1")],
            aliases: BTreeMap::new(),
            init: Vec::new(),
        };
        assert_eq!(
            render_rc(&data, &BlameSet::default()),
            "# >>> confit:unknown\nexport A=1\n# <<< confit\n"
        );
    }

    #[test]
    fn same_tool_entries_share_one_section() {
        let data = RcData {
            profile: Vec::new(),
            env: vec![env("A", "1"), env("B", "2"), env("C", "3")],
            aliases: BTreeMap::new(),
            init: Vec::new(),
        };
        let blame = BlameSet {
            env: vec!["x".into(), "y".into(), "x".into()],
            ..BlameSet::default()
        };
        assert_eq!(
            render_rc(&data, &blame),
            "# >>> confit:x\nexport A=1\nexport C=3\n# <<< confit\n\
             # >>> confit:y\nexport B=2\n# <<< confit\n"
        );
    }
}
