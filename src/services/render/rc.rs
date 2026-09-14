//! Rc Rendering
//!
//! Shell rc text rendering with bare entry lines.

use crate::model::state::condition::Condition;
use crate::model::state::rc::AliasEntry;
use crate::model::state::rc::EnvEntry;
use crate::model::state::rc::InitEntry;
use crate::model::state::rc::InitSpec;
use crate::model::state::rc::ProfileEntry;
use crate::model::state::rc::RcData;

use crate::model::state::rc::Lane;

use super::shell_escape::escape_argv;

/// Renders rc data to shell text with bare entry lines.
///
/// Setup holds profile plus env plus first lane init lines. One guard
/// follows setup while alias or final lines follow. Aliases hold the
/// third block. Middle lane init lines plus last lane init lines hold
/// the final block. Blocks join with one blank line.
///
/// # Arguments
///
/// * `data` - the merged per-shell entries.
///
/// # Returns
///
/// Shell text with trailing newline. Empty data yields the empty string.
/// Setup only output carries no guard.
///
/// # Examples
/// ```rust
/// use confit::model::state::level::Level;
/// use confit::model::state::rc::AliasEntry;
/// use confit::model::state::rc::AliasSpec;
/// use confit::model::state::rc::EnvEntry;
/// use confit::model::state::rc::EnvSpec;
/// use confit::model::state::rc::RcData;
/// use confit::services::render::rc::render_rc;
///
/// let data = RcData {
///     profile: Vec::new(),
///     env: vec![EnvEntry { spec: EnvSpec { name: "A".into(), value: "1".into() }, when: None, priority: Level::Normal }],
///     aliases: vec![AliasEntry { spec: AliasSpec { name: "cat".into(), value: "bat".into() }, when: None, priority: Level::Normal }],
///     init: Vec::new(),
/// };
/// let rendered = render_rc(&data);
/// assert!(rendered.contains("export A=1"));
/// assert!(rendered.contains("case $- in"));
/// assert!(rendered.contains("alias cat=bat"));
/// assert!(matches!((rendered.find("export A=1"), rendered.find("case $- in")), (Some(a), Some(b)) if a < b));
/// assert!(matches!((rendered.find("case $- in"), rendered.find("alias cat=bat")), (Some(a), Some(b)) if a < b));
/// ```
pub fn render_rc(data: &RcData) -> String {
    let setup = setup_lines(data);
    let aliases = alias_lines(data);
    let final_block = final_lines(data);
    let mut blocks: Vec<String> = Vec::new();
    if !setup.is_empty() {
        blocks.push(setup.join("\n"));
    }
    if !aliases.is_empty() || !final_block.is_empty() {
        blocks.push(guard_text().to_string());
    }
    if !aliases.is_empty() {
        blocks.push(aliases.join("\n"));
    }
    if !final_block.is_empty() {
        blocks.push(final_block.join("\n"));
    }
    if blocks.is_empty() {
        return String::new();
    }
    let mut out = blocks.join("\n\n");
    out.push('\n');
    out
}

/// Collects setup lines in render order.
///
/// # Arguments
///
/// * `data` - the merged per-shell entries.
///
/// # Returns
///
/// Profile lines plus env lines plus first lane init lines in listed order.
fn setup_lines(data: &RcData) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in &data.profile {
        out.push(render_profile_entry(entry));
    }
    for entry in &data.env {
        out.push(render_env_entry(entry));
    }
    for entry in &data.init {
        if init_lane(entry) == Lane::First {
            out.push(render_init_entry(entry));
        }
    }
    out
}

/// Reports the interactivity guard text.
///
/// # Returns
///
/// The guard block text shared by every shell file.
fn guard_text() -> &'static str {
    "case $- in\n*i*) ;;\n*) return ;;\nesac"
}

/// Collects alias lines in listed order.
///
/// # Arguments
///
/// * `data` - the merged per-shell entries.
///
/// # Returns
///
/// Alias lines in listed order.
fn alias_lines(data: &RcData) -> Vec<String> {
    data.aliases.iter().map(render_alias_line).collect()
}

/// Collects final init lines in lane order.
///
/// # Arguments
///
/// * `data` - the merged per-shell entries.
///
/// # Returns
///
/// Middle lane init lines in listed order plus last lane init lines in listed order.
fn final_lines(data: &RcData) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in &data.init {
        if init_lane(entry) == Lane::Middle {
            out.push(render_init_entry(entry));
        }
    }
    for entry in &data.init {
        if init_lane(entry) == Lane::Last {
            out.push(render_init_entry(entry));
        }
    }
    out
}

/// Reports the execution lane of one init entry.
///
/// # Arguments
///
/// * `entry` - the init entry under inspection.
///
/// # Returns
///
/// The lane carried by the entry spec.
fn init_lane(entry: &InitEntry) -> Lane {
    match &entry.spec {
        InitSpec::Eval { lane, .. }
        | InitSpec::Cmd { lane, .. }
        | InitSpec::Source { lane, .. } => *lane,
    }
}

/// Renders one env entry as an export line.
///
/// # Arguments
///
/// * `entry` - the env entry.
///
/// # Returns
///
/// Shell export line for the entry, guarded by the entry condition.
fn render_env_entry(entry: &EnvEntry) -> String {
    let value = escape_argv(std::slice::from_ref(&entry.spec.value));
    wrap_conditional(
        format!("export {}={value}", entry.spec.name),
        entry.when.as_ref(),
    )
}

/// Renders one profile entry as an export line.
///
/// # Arguments
///
/// * `entry` - the profile entry.
///
/// # Returns
///
/// Shell export line for the entry, guarded by the entry condition.
fn render_profile_entry(entry: &ProfileEntry) -> String {
    let value = escape_argv(std::slice::from_ref(&entry.spec.value));
    let line = match entry.spec.op {
        crate::model::state::rc::PathOp::Prepend => {
            format!(
                "export {}={value}:\"${{{}}}\"",
                entry.spec.name, entry.spec.name
            )
        }
        crate::model::state::rc::PathOp::Append => {
            format!(
                "export {}=\"${{{}}}\":{value}",
                entry.spec.name, entry.spec.name
            )
        }
    };
    wrap_conditional(line, entry.when.as_ref())
}

/// Renders one alias as an alias line.
///
/// # Arguments
///
/// * `entry` - the alias entry.
///
/// # Returns
///
/// Shell alias line for the entry, guarded by the entry condition.
fn render_alias_line(entry: &AliasEntry) -> String {
    let quoted = escape_argv(std::slice::from_ref(&entry.spec.value));
    wrap_conditional(
        format!("alias {}={quoted}", entry.spec.name),
        entry.when.as_ref(),
    )
}

/// Renders one init entry as shell text.
///
/// # Arguments
///
/// * `entry` - the init entry.
///
/// # Returns
///
/// Shell line for the entry, guarded by the entry condition.
fn render_init_entry(entry: &InitEntry) -> String {
    match &entry.spec {
        InitSpec::Eval { argv, .. } => wrap_conditional(
            format!("eval \"$({})\"", escape_argv(argv)),
            entry.when.as_ref(),
        ),
        InitSpec::Cmd { argv, .. } => wrap_conditional(escape_argv(argv), entry.when.as_ref()),
        InitSpec::Source { path, .. } => wrap_conditional(
            format!("source {}", escape_argv(std::slice::from_ref(path))),
            entry.when.as_ref(),
        ),
    }
}

/// Renders one condition as a Bourne test string.
fn render_condition(condition: &Condition) -> String {
    match condition {
        Condition::EnvEq { key, value } => {
            let rhs = escape_argv(std::slice::from_ref(value));
            format!("[ \"${{{key}}}\" = {rhs} ]")
        }
        Condition::EnvSet { key } => format!("[ -n \"${{{key}}}\" ]"),
        Condition::InPath { name } => {
            let binary = escape_argv(std::slice::from_ref(name));
            format!("command -v {binary} >/dev/null 2>&1")
        }
        Condition::Exists { path } => {
            let candidate = escape_argv(std::slice::from_ref(path));
            format!("[ -e {candidate} ]")
        }
        Condition::All(items) if items.is_empty() => "true".to_string(),
        Condition::Any(items) if items.is_empty() => "false".to_string(),
        Condition::All(items) => join_conditions(items, "&&"),
        Condition::Any(items) => join_conditions(items, "||"),
        Condition::Not(inner) => match inner.as_ref() {
            Condition::All(_) | Condition::Any(_) | Condition::Not(_) => {
                format!("! ( {} )", render_condition(inner))
            }
            _ => format!("! {}", render_condition(inner)),
        },
    }
}

/// Joins nested conditions under one operator, grouping mixed shapes.
fn join_conditions(items: &[Condition], op: &str) -> String {
    items
        .iter()
        .map(|item| group_condition(item, op))
        .collect::<Vec<_>>()
        .join(&format!(" {op} "))
}

/// Groups one nested condition for joining under an operator.
fn group_condition(item: &Condition, parent_op: &str) -> String {
    let rendered = render_condition(item);
    match item {
        Condition::All(_) if parent_op != "&&" => format!("( {rendered} )"),
        Condition::Any(_) if parent_op != "||" => format!("( {rendered} )"),
        _ => rendered,
    }
}

/// Wraps one rendered line in a condition guard, passing absence through.
fn wrap_conditional(line: String, when: Option<&Condition>) -> String {
    match when {
        None => line,
        Some(condition) => format!("if {}; then\n  {line}\nfi", render_condition(condition)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::model::state::level::Level;
    use crate::model::state::rc::AliasSpec;
    use crate::model::state::rc::EnvSpec;
    use crate::model::state::rc::Lane;
    use crate::model::state::rc::ProfileSpec;

    fn env(name: &str, value: &str) -> EnvEntry {
        EnvEntry {
            spec: EnvSpec {
                name: name.into(),
                value: value.into(),
            },
            when: None,
            priority: Level::Normal,
        }
    }

    fn alias(name: &str, value: &str) -> AliasEntry {
        AliasEntry {
            spec: AliasSpec {
                name: name.into(),
                value: value.into(),
            },
            when: None,
            priority: Level::Normal,
        }
    }

    fn guarded_alias(name: &str, value: &str, when: Condition) -> AliasEntry {
        AliasEntry {
            spec: AliasSpec {
                name: name.into(),
                value: value.into(),
            },
            when: Some(when),
            priority: Level::Normal,
        }
    }

    fn guarded_init(entry: InitEntry, when: Condition) -> InitEntry {
        InitEntry {
            when: Some(when),
            ..entry
        }
    }

    fn profile(value: &str, op: crate::model::state::rc::PathOp) -> ProfileEntry {
        ProfileEntry {
            spec: ProfileSpec {
                name: "PATH".into(),
                value: value.into(),
                op,
            },
            when: None,
            priority: Level::Normal,
        }
    }

    fn guarded_env(name: &str, value: &str, when: Condition) -> EnvEntry {
        EnvEntry {
            spec: EnvSpec {
                name: name.into(),
                value: value.into(),
            },
            when: Some(when),
            priority: Level::Normal,
        }
    }

    #[test]
    fn condition_renders_every_variant() {
        assert_eq!(
            render_condition(&Condition::EnvEq {
                key: "A".into(),
                value: "b".into()
            }),
            r#"[ "${A}" = b ]"#
        );
        assert_eq!(
            render_condition(&Condition::EnvEq {
                key: "A".into(),
                value: "hello world".into()
            }),
            r#"[ "${A}" = 'hello world' ]"#
        );
        assert_eq!(
            render_condition(&Condition::EnvSet {
                key: "SSH_TTY".into()
            }),
            r#"[ -n "${SSH_TTY}" ]"#
        );
        assert_eq!(
            render_condition(&Condition::InPath { name: "bat".into() }),
            "command -v bat >/dev/null 2>&1"
        );
        assert_eq!(
            render_condition(&Condition::InPath {
                name: "my tool".into()
            }),
            "command -v 'my tool' >/dev/null 2>&1"
        );
        assert_eq!(
            render_condition(&Condition::Exists {
                path: "/data/seed".into()
            }),
            "[ -e /data/seed ]"
        );
        assert_eq!(
            render_condition(&Condition::Exists {
                path: "~/my dir".into()
            }),
            "[ -e '~/my dir' ]"
        );
    }

    #[test]
    fn condition_combines_and_negates() {
        let warp = Condition::EnvEq {
            key: "TERM_PROGRAM".into(),
            value: "WarpTerminal".into(),
        };
        let ssh = Condition::EnvSet {
            key: "SSH_TTY".into(),
        };
        assert_eq!(
            render_condition(&Condition::All(vec![warp.clone(), ssh.clone()])),
            r#"[ "${TERM_PROGRAM}" = WarpTerminal ] && [ -n "${SSH_TTY}" ]"#
        );
        assert_eq!(
            render_condition(&Condition::Any(vec![warp.clone(), ssh.clone()])),
            r#"[ "${TERM_PROGRAM}" = WarpTerminal ] || [ -n "${SSH_TTY}" ]"#
        );
        assert_eq!(
            render_condition(&Condition::Not(Box::new(ssh.clone()))),
            r#"! [ -n "${SSH_TTY}" ]"#
        );
        assert_eq!(
            render_condition(&Condition::Not(Box::new(Condition::All(vec![
                warp.clone(),
                ssh.clone()
            ])))),
            r#"! ( [ "${TERM_PROGRAM}" = WarpTerminal ] && [ -n "${SSH_TTY}" ] )"#
        );
        assert_eq!(
            render_condition(&Condition::All(vec![
                warp.clone(),
                Condition::Any(vec![ssh.clone(), warp.clone()]),
            ])),
            r#"[ "${TERM_PROGRAM}" = WarpTerminal ] && ( [ -n "${SSH_TTY}" ] || [ "${TERM_PROGRAM}" = WarpTerminal ] )"#
        );
        assert_eq!(
            render_condition(&Condition::Any(vec![
                ssh.clone(),
                Condition::All(vec![warp.clone(), ssh.clone()]),
            ])),
            r#"[ -n "${SSH_TTY}" ] || ( [ "${TERM_PROGRAM}" = WarpTerminal ] && [ -n "${SSH_TTY}" ] )"#
        );
        assert_eq!(render_condition(&Condition::All(Vec::new())), "true");
        assert_eq!(render_condition(&Condition::Any(Vec::new())), "false");
    }

    #[test]
    fn conditional_env_wraps_while_plain_passes_through() {
        let data = RcData {
            profile: Vec::new(),
            env: vec![guarded_env(
                "A",
                "1",
                Condition::EnvSet {
                    key: "SSH_TTY".into(),
                },
            )],
            aliases: Vec::new(),
            init: Vec::new(),
        };
        assert_eq!(
            render_rc(&data),
            "if [ -n \"${SSH_TTY}\" ]; then\n  export A=1\nfi\n"
        );
    }

    #[test]
    fn conditional_profile_wraps_init_line() {
        let data = RcData {
            profile: vec![ProfileEntry {
                spec: ProfileSpec {
                    name: "PATH".into(),
                    value: "/a".into(),
                    op: crate::model::state::rc::PathOp::Prepend,
                },
                when: Some(Condition::InPath { name: "bat".into() }),
                priority: Level::Normal,
            }],
            env: Vec::new(),
            aliases: Vec::new(),
            init: Vec::new(),
        };
        assert_eq!(
            render_rc(&data),
            "if command -v bat >/dev/null 2>&1; then\n  export PATH=/a:\"${PATH}\"\nfi\n"
        );
    }

    #[test]
    fn conditional_alias_wraps_while_plain_passes_through() {
        let data = RcData {
            profile: Vec::new(),
            env: Vec::new(),
            aliases: vec![
                alias("ls", "eza"),
                guarded_alias("cat", "bat", Condition::InPath { name: "bat".into() }),
            ],
            init: Vec::new(),
        };
        assert_eq!(
            render_rc(&data),
            "case $- in\n*i*) ;;\n*) return ;;\nesac\n\nalias ls=eza\nif command -v bat >/dev/null 2>&1; then\n  alias cat=bat\nfi\n"
        );
    }

    #[test]
    fn conditional_init_wraps_each_shape() {
        let guard = Condition::EnvSet {
            key: "SSH_TTY".into(),
        };
        let data = RcData {
            profile: Vec::new(),
            env: Vec::new(),
            aliases: Vec::new(),
            init: vec![
                guarded_init(
                    InitEntry {
                        spec: InitSpec::Cmd {
                            argv: vec!["task".into(), "--completion".into()],
                            lane: Lane::Middle,
                        },
                        when: None,
                        priority: Level::Normal,
                    },
                    guard.clone(),
                ),
                guarded_init(
                    InitEntry {
                        spec: InitSpec::Source {
                            path: "~/.cargo/env".into(),
                            lane: Lane::Middle,
                        },
                        when: None,
                        priority: Level::Normal,
                    },
                    guard.clone(),
                ),
            ],
        };
        assert_eq!(
            render_rc(&data),
            "case $- in\n*i*) ;;\n*) return ;;\nesac\n\nif [ -n \"${SSH_TTY}\" ]; then\n  task --completion\nfi\nif [ -n \"${SSH_TTY}\" ]; then\n  source '~/.cargo/env'\nfi\n"
        );
    }

    #[test]
    fn wrap_helper_covers_alias_and_init_lines() {
        let guard = Condition::Exists {
            path: "/data/seed".into(),
        };
        assert_eq!(
            wrap_conditional("alias cat=bat".to_string(), None),
            "alias cat=bat"
        );
        assert_eq!(
            wrap_conditional("alias cat=bat".to_string(), Some(&guard)),
            "if [ -e /data/seed ]; then\n  alias cat=bat\nfi"
        );
        assert_eq!(
            wrap_conditional("task --completion".to_string(), Some(&guard)),
            "if [ -e /data/seed ]; then\n  task --completion\nfi"
        );
    }

    #[test]
    fn empty_data_renders_empty_string() {
        assert_eq!(render_rc(&RcData::default()), "");
    }

    #[test]
    fn env_lines_share_one_section() {
        let data = RcData {
            profile: Vec::new(),
            env: vec![env("A", "1"), env("B", "x y")],
            aliases: Vec::new(),
            init: Vec::new(),
        };
        let rendered = render_rc(&data);
        assert_eq!(rendered, "export A=1\nexport B='x y'\n");
    }

    #[test]
    fn three_blocks_join_with_blank_lines() {
        let data = RcData {
            profile: Vec::new(),
            env: vec![env("A", "1")],
            aliases: vec![alias("cat", "bat")],
            init: vec![InitEntry {
                spec: InitSpec::Cmd {
                    argv: vec!["task".into(), "--completion".into()],
                    lane: Lane::Middle,
                },
                when: None,
                priority: Level::Normal,
            }],
        };
        let rendered = render_rc(&data);
        assert_eq!(
            rendered,
            "export A=1\n\
             \n\
             case $- in\n\
             *i*) ;;\n\
             *) return ;;\n\
             esac\n\
             \n\
             alias cat=bat\n\
             \n\
             task --completion\n"
        );
    }

    #[test]
    fn profile_prepends_before_env_in_section() {
        let data = RcData {
            profile: vec![profile("/a", crate::model::state::rc::PathOp::Prepend)],
            env: vec![env("A", "1")],
            aliases: Vec::new(),
            init: Vec::new(),
        };
        let rendered = render_rc(&data);
        assert_eq!(rendered, "export PATH=/a:\"${PATH}\"\nexport A=1\n");
    }

    #[test]
    fn profile_append_renders_value_last() {
        let data = RcData {
            profile: vec![profile("/z", crate::model::state::rc::PathOp::Append)],
            env: Vec::new(),
            aliases: Vec::new(),
            init: Vec::new(),
        };
        assert_eq!(render_rc(&data), "export PATH=\"${PATH}\":/z\n");
    }

    #[test]
    fn eval_wraps_escaped_argv() {
        let data = RcData {
            profile: Vec::new(),
            env: Vec::new(),
            aliases: Vec::new(),
            init: vec![
                InitEntry {
                    spec: InitSpec::Eval {
                        argv: vec!["zoxide".into(), "init".into(), "bash".into()],
                        lane: Lane::Middle,
                    },
                    when: None,
                    priority: Level::Normal,
                },
                InitEntry {
                    spec: InitSpec::Eval {
                        argv: vec!["echo".into(), "a b".into()],
                        lane: Lane::Middle,
                    },
                    when: None,
                    priority: Level::Normal,
                },
            ],
        };
        assert_eq!(
            render_rc(&data),
            "case $- in\n*i*) ;;\n*) return ;;\nesac\n\neval \"$(zoxide init bash)\"\neval \"$(echo 'a b')\"\n"
        );
    }

    #[test]
    fn source_renders_plain_path() {
        let data = RcData {
            profile: Vec::new(),
            env: Vec::new(),
            aliases: Vec::new(),
            init: vec![InitEntry {
                spec: InitSpec::Source {
                    path: "~/.cargo/env".into(),
                    lane: Lane::Middle,
                },
                when: None,
                priority: Level::Normal,
            }],
        };
        assert_eq!(
            render_rc(&data),
            "case $- in\n*i*) ;;\n*) return ;;\nesac\n\nsource '~/.cargo/env'\n"
        );
    }

    #[test]
    fn single_section_holds_every_env_line() {
        let data = RcData {
            profile: Vec::new(),
            env: vec![env("A", "1")],
            aliases: Vec::new(),
            init: Vec::new(),
        };
        assert_eq!(render_rc(&data), "export A=1\n");
    }

    #[test]
    fn entries_share_one_section_in_order() {
        let data = RcData {
            profile: Vec::new(),
            env: vec![env("A", "1"), env("B", "2"), env("C", "3")],
            aliases: Vec::new(),
            init: Vec::new(),
        };
        assert_eq!(render_rc(&data), "export A=1\nexport B=2\nexport C=3\n");
    }

    #[test]
    fn first_lane_init_sits_before_guard() {
        let data = RcData {
            profile: Vec::new(),
            env: vec![env("A", "1")],
            aliases: vec![alias("cat", "bat")],
            init: vec![
                InitEntry {
                    spec: InitSpec::Cmd {
                        argv: vec!["nim".into(), "init".into()],
                        lane: Lane::First,
                    },
                    when: None,
                    priority: Level::Normal,
                },
                InitEntry {
                    spec: InitSpec::Cmd {
                        argv: vec!["task".into(), "--completion".into()],
                        lane: Lane::Middle,
                    },
                    when: None,
                    priority: Level::Normal,
                },
            ],
        };
        assert_eq!(
            render_rc(&data),
            "export A=1\nnim init\n\ncase $- in\n*i*) ;;\n*) return ;;\nesac\n\nalias cat=bat\n\ntask --completion\n"
        );
    }

    #[test]
    fn last_lane_init_follows_middle() {
        let data = RcData {
            profile: Vec::new(),
            env: Vec::new(),
            aliases: vec![alias("cat", "bat")],
            init: vec![
                InitEntry {
                    spec: InitSpec::Cmd {
                        argv: vec!["sdkman".into(), "init".into()],
                        lane: Lane::Last,
                    },
                    when: None,
                    priority: Level::Normal,
                },
                InitEntry {
                    spec: InitSpec::Cmd {
                        argv: vec!["task".into(), "--completion".into()],
                        lane: Lane::Middle,
                    },
                    when: None,
                    priority: Level::Normal,
                },
            ],
        };
        assert_eq!(
            render_rc(&data),
            "case $- in\n*i*) ;;\n*) return ;;\nesac\n\nalias cat=bat\n\ntask --completion\nsdkman init\n"
        );
    }

    #[test]
    fn setup_only_omits_guard() {
        let data = RcData {
            profile: vec![profile("/a", crate::model::state::rc::PathOp::Prepend)],
            env: vec![env("A", "1")],
            aliases: Vec::new(),
            init: vec![InitEntry {
                spec: InitSpec::Cmd {
                    argv: vec!["nim".into(), "init".into()],
                    lane: Lane::First,
                },
                when: None,
                priority: Level::Normal,
            }],
        };
        let rendered = render_rc(&data);
        assert!(!rendered.contains("case $- in"));
        assert_eq!(
            rendered,
            "export PATH=/a:\"${PATH}\"\nexport A=1\nnim init\n"
        );
    }
}
