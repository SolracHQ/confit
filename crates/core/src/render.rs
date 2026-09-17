//! Render
//!
//! Document payloads to on-disk bytes.

use std::borrow::Cow;

use crate::document::{
    Condition, Document, DocumentData, RcData, RcEntry, RcOp, StructuredFormat, Table,
};
use crate::error::{Error, Result};

/// Interactivity guard shared by every shell file.
const GUARD: &str = "case $- in\n*i*) ;;\n*) return ;;\nesac";

impl Document {
    /// Renders one document to exact on-disk bytes.
    ///
    /// Structured payloads serialize through their format.
    /// Text payloads pass content through. Link payloads pass the
    /// target through. Rc payloads render shell text. Opaque
    /// plus tree payloads fail, reads use `bytes` instead.
    ///
    /// # Returns
    ///
    /// Exact bytes landing on disk for the document.
    ///
    /// # Errors
    ///
    /// Opaque payloads fail as plan errors. Serializer failures
    /// fail as plan errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{Document, DocumentData};
    /// use confit_core::ids::DocPath;
    ///
    /// let document = Document::new(
    ///     DocPath::new("note"),
    ///     DocumentData::Text { content: "hi".into(), mode: None },
    /// );
    /// assert!(matches!(document.render(), Ok(bytes) if bytes == b"hi".to_vec()));
    /// ```
    pub fn render(&self) -> Result<Vec<u8>> {
        match &self.data {
            DocumentData::Structured { format, data } => match format {
                StructuredFormat::Toml => Ok(render_toml(data)?.into_bytes()),
                StructuredFormat::Json => Ok(render_json(data)?.into_bytes()),
                StructuredFormat::Yaml => Ok(render_yaml(data)?.into_bytes()),
            },
            DocumentData::Text { content, .. } => Ok(content.as_bytes().to_vec()),
            DocumentData::Link { target } => Ok(target.as_bytes().to_vec()),
            DocumentData::Rc(data) => Ok(render_rc(data).into_bytes()),
            DocumentData::Opaque { .. } => Err(Error::Plan(
                "render opaque: opaque documents hold raw bytes".to_string(),
            )),
            DocumentData::Tree { .. } => Err(Error::Plan(
                "render tree: tree documents hold member bytes".to_string(),
            )),
        }
    }

    /// Returns exact on-disk bytes for one document.
    ///
    /// Opaque payloads return raw bytes. Tree payloads return
    /// canonical manifest bytes for hashing, never disk bytes.
    /// Every other payload renders through `render`.
    ///
    /// # Returns
    ///
    /// Exact bytes landing on disk for the document.
    ///
    /// # Errors
    ///
    /// Serializer failures fail as plan errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{Document, DocumentData};
    /// use confit_core::ids::DocPath;
    ///
    /// let document = Document::new(
    ///     DocPath::new("bin"),
    ///     DocumentData::Opaque { content: vec![0xFF, 0x00], mode: None },
    /// );
    /// assert!(matches!(document.bytes(), Ok(bytes) if bytes == vec![0xFF, 0x00]));
    /// ```
    pub fn bytes(&self) -> Result<Vec<u8>> {
        match &self.data {
            DocumentData::Opaque { content, .. } => Ok(content.clone()),
            DocumentData::Tree { members } => Ok(crate::document::tree_manifest_bytes(members)),
            _ => self.render(),
        }
    }
}

/// Renders a table to TOML text.
fn render_toml(table: &Table) -> Result<String> {
    toml::to_string(table).map_err(|error| Error::Plan(format!("render toml: {error}")))
}

/// Renders a table to pretty JSON text.
fn render_json(table: &Table) -> Result<String> {
    serde_json::to_string_pretty(table)
        .map_err(|error| Error::Plan(format!("render json: {error}")))
}

/// Renders a table to YAML text.
fn render_yaml(table: &Table) -> Result<String> {
    noyalib::to_string(table).map_err(|error| Error::Plan(format!("render yaml: {error}")))
}

/// Renders rc data to shell text with trailing newline.
fn render_rc(data: &RcData) -> String {
    let profile = section_lines(&data.profile);
    let config = section_lines(&data.config);
    let finals = section_lines(&data.final_entries);
    let mut blocks: Vec<String> = Vec::new();
    if !profile.is_empty() {
        blocks.push(profile.join("\n"));
    }
    if !data.config.is_empty() || !data.final_entries.is_empty() {
        blocks.push(GUARD.to_string());
    }
    if !config.is_empty() {
        blocks.push(config.join("\n"));
    }
    if !finals.is_empty() {
        blocks.push(finals.join("\n"));
    }
    if blocks.is_empty() {
        return String::new();
    }
    let mut out = blocks.join("\n\n");
    out.push('\n');
    out
}

/// Collects one section lines in declaration order.
fn section_lines(entries: &[RcEntry]) -> Vec<String> {
    entries.iter().flat_map(render_entry).collect()
}

/// Renders one rc entry as shell lines.
fn render_entry(entry: &RcEntry) -> Vec<String> {
    let line = match &entry.op {
        RcOp::Env { name, value } => {
            let exported = escape_argv(std::slice::from_ref(value));
            format!("export {name}={exported}")
        }
        RcOp::Path { name, dir, .. } => {
            let placed = escape_argv(std::slice::from_ref(dir));
            format!("export {name}={placed}:\"${{{name}}}\"")
        }
        RcOp::Alias { name, expansion } => {
            let expanded = escape_argv(std::slice::from_ref(expansion));
            format!("alias {name}={expanded}")
        }
        RcOp::Eval { argv, .. } => format!("eval \"$({})\"", escape_argv(argv)),
        RcOp::Cmd { argv, .. } => escape_argv(argv),
        RcOp::Source { path, .. } => {
            format!("source {}", escape_argv(std::slice::from_ref(path)))
        }
    };
    match entry.when.as_ref() {
        None => vec![line],
        Some(guard) => vec![
            format!("if {}; then", render_guard(guard)),
            format!("  {line}"),
            "fi".to_string(),
        ],
    }
}

/// Renders one condition as a Bourne test string.
fn render_guard(guard: &Condition) -> String {
    match guard {
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
        Condition::All(items) => join_guards(items, "&&"),
        Condition::Any(items) => join_guards(items, "||"),
        Condition::Not(inner) => match inner.as_ref() {
            Condition::All(_) | Condition::Any(_) | Condition::Not(_) => {
                format!("! ( {} )", render_guard(inner))
            }
            _ => format!("! {}", render_guard(inner)),
        },
    }
}

/// Joins nested guards under one operator, grouping mixed shapes.
fn join_guards(items: &[Condition], op: &str) -> String {
    items
        .iter()
        .map(|item| group_guard(item, op))
        .collect::<Vec<_>>()
        .join(&format!(" {op} "))
}

/// Groups one nested guard for joining under an operator.
fn group_guard(item: &Condition, parent: &str) -> String {
    let rendered = render_guard(item);
    match item {
        Condition::All(_) if parent != "&&" => format!("( {rendered} )"),
        Condition::Any(_) if parent != "||" => format!("( {rendered} )"),
        _ => rendered,
    }
}

/// Renders argv as one shell line with Bourne quoting.
fn escape_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| quote_word(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Quotes one word, borrowing safe words intact.
fn quote_word(word: &str) -> Cow<'_, str> {
    if !word.is_empty() && word.bytes().all(is_safe_byte) {
        Cow::Borrowed(word)
    } else if word.is_empty() {
        Cow::Borrowed("''")
    } else {
        Cow::Owned(format!("'{}'", word.replace('\'', "'\\''")))
    }
}

/// Reports whether a byte passes through unquoted.
fn is_safe_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'_'
            | b'@'
            | b'%'
            | b'+'
            | b'='
            | b':'
            | b','
            | b'.'
            | b'/'
            | b'-'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{PathOp, RcEntry, RcOp};
    use crate::ids::DocPath;

    fn rc_document(data: RcData) -> Document {
        Document::new(DocPath::new("~/.bashrc"), DocumentData::Rc(data))
    }

    fn entry(op: RcOp, when: Option<Condition>) -> RcEntry {
        RcEntry { op, when }
    }

    fn env(name: &str, value: &str) -> RcEntry {
        entry(
            RcOp::Env {
                name: name.to_string(),
                value: value.to_string(),
            },
            None,
        )
    }

    fn alias(name: &str, expansion: &str) -> RcEntry {
        entry(
            RcOp::Alias {
                name: name.to_string(),
                expansion: expansion.to_string(),
            },
            None,
        )
    }

    fn path(dir: &str) -> RcEntry {
        entry(
            RcOp::Path {
                name: "PATH".to_string(),
                dir: dir.to_string(),
                op: PathOp::Prepend,
            },
            None,
        )
    }

    fn eval(argv: &[&str]) -> RcEntry {
        entry(
            RcOp::Eval {
                argv: argv.iter().map(|item| (*item).to_string()).collect(),
            },
            None,
        )
    }

    fn cmd(argv: &[&str]) -> RcEntry {
        entry(
            RcOp::Cmd {
                argv: argv.iter().map(|item| (*item).to_string()).collect(),
            },
            None,
        )
    }

    #[test]
    fn rc_golden_layout_with_guards() {
        let data = RcData::new(
            vec![path("/home/u/.local/bin"), env("EDITOR", "hx")],
            vec![
                alias("ll", "ls -l"),
                entry(
                    RcOp::Alias {
                        name: "cat".to_string(),
                        expansion: "bat".to_string(),
                    },
                    Some(Condition::InPath { name: "bat".into() }),
                ),
            ],
            vec![
                eval(&["mise", "activate", "bash"]),
                eval(&["starship", "init", "bash"]),
                cmd(&["sdkman", "init"]),
            ],
        );
        let bytes = match rc_document(data).render() {
            Ok(bytes) => bytes,
            Err(error) => panic!("rc renders: {error}"),
        };
        assert_eq!(
            String::from_utf8_lossy(&bytes),
            "export PATH=/home/u/.local/bin:\"${PATH}\"\n\
             export EDITOR=hx\n\
             \n\
             case $- in\n\
             *i*) ;;\n\
             *) return ;;\n\
             esac\n\
             \n\
             alias ll='ls -l'\n\
             if command -v bat >/dev/null 2>&1; then\n\
             \x20 alias cat=bat\n\
             fi\n\
             \n\
             eval \"$(mise activate bash)\"\n\
             eval \"$(starship init bash)\"\n\
             sdkman init\n"
        );
    }

    #[test]
    fn setup_only_skips_guard() {
        let data = RcData::new(vec![path("/a")], Vec::new(), Vec::new());
        let bytes = match rc_document(data).render() {
            Ok(bytes) => bytes,
            Err(error) => panic!("rc renders: {error}"),
        };
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("case $- in"));
        assert_eq!(text, "export PATH=/a:\"${PATH}\"\n");
    }

    #[test]
    fn alias_in_profile_renders_in_setup_block() {
        let data = RcData::new(
            vec![alias("ll", "ls -l"), env("EDITOR", "hx")],
            Vec::new(),
            Vec::new(),
        );
        let bytes = match rc_document(data).render() {
            Ok(bytes) => bytes,
            Err(error) => panic!("rc renders: {error}"),
        };
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("case $- in"));
        assert_eq!(text, "alias ll='ls -l'\nexport EDITOR=hx\n");
    }

    #[test]
    fn env_in_final_renders_in_final_block() {
        let data = RcData::new(Vec::new(), Vec::new(), vec![env("EDITOR", "hx")]);
        let bytes = match rc_document(data).render() {
            Ok(bytes) => bytes,
            Err(error) => panic!("rc renders: {error}"),
        };
        assert_eq!(
            String::from_utf8_lossy(&bytes),
            "case $- in\n*i*) ;;\n*) return ;;\nesac\n\nexport EDITOR=hx\n"
        );
    }

    #[test]
    fn mixed_section_keeps_declaration_order() {
        let data = RcData::new(
            vec![cmd(&["zzz"]), env("EDITOR", "hx"), eval(&["aaa"])],
            Vec::new(),
            vec![cmd(&["z-last"]), eval(&["a-first"]), cmd(&["m-mid"])],
        );
        let bytes = match rc_document(data).render() {
            Ok(bytes) => bytes,
            Err(error) => panic!("rc renders: {error}"),
        };
        assert_eq!(
            String::from_utf8_lossy(&bytes),
            "zzz\n\
             export EDITOR=hx\n\
             eval \"$(aaa)\"\n\
             \n\
             case $- in\n\
             *i*) ;;\n\
             *) return ;;\n\
             esac\n\
             \n\
             z-last\n\
             eval \"$(a-first)\"\n\
             m-mid\n"
        );
    }

    #[test]
    fn null_toml_value_fails_as_plan_error() {
        let data: Table = [("name".to_string(), serde_json::Value::Null)]
            .into_iter()
            .collect();
        let error = match render_toml(&data) {
            Ok(_) => panic!("null toml renders"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Plan(_)));
    }
}
