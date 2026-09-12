//! Shell Escape
//!
//! Bourne shell argument quoting for rendered rc lines.

/// Renders argv as one shell line with Bourne quoting.
///
/// # Arguments
///
/// * `argv` - the command plus arguments, in order.
///
/// # Returns
///
/// Single shell line with arguments joined by spaces.
///
/// # Examples
/// ```rust
/// use confit::services::render::shell_escape::escape_argv;
///
/// let argv = vec!["zoxide".to_string(), "init".to_string(), "bash".to_string()];
/// assert_eq!(escape_argv(&argv), "zoxide init bash");
/// let spaced = vec!["echo".to_string(), "hello world".to_string()];
/// assert_eq!(escape_argv(&spaced), "echo 'hello world'");
/// ```
pub fn escape_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| quote_word(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Quotes one word for shell use.
///
/// # Arguments
///
/// * `word` - the word under quoting.
///
/// # Returns
///
/// Bare word for safe input; single-quoted text otherwise.
fn quote_word(word: &str) -> String {
    if !word.is_empty() && word.bytes().all(is_safe_byte) {
        word.to_string()
    } else if word.is_empty() {
        "''".to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// Checks whether a byte passes through unquoted.
///
/// # Arguments
///
/// * `byte` - the byte under test.
///
/// # Returns
///
/// `true` for bytes passing through unquoted.
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

    #[test]
    fn empty_argv_renders_empty_line() {
        let argv: Vec<String> = Vec::new();
        assert_eq!(escape_argv(&argv), "");
    }

    #[test]
    fn safe_words_pass_through_bare() {
        let argv = vec![
            "zoxide".to_string(),
            "init".to_string(),
            "bash".to_string(),
            "--flag".to_string(),
            "a/b:c,d@e%f+g=h-i".to_string(),
        ];
        assert_eq!(
            escape_argv(&argv),
            "zoxide init bash --flag a/b:c,d@e%f+g=h-i"
        );
    }

    #[test]
    fn spacing_and_specials_quote() {
        let argv = vec!["echo".to_string(), "hello world".to_string()];
        assert_eq!(escape_argv(&argv), "echo 'hello world'");
        let argv = vec!["echo".to_string(), "$HOME".to_string()];
        assert_eq!(escape_argv(&argv), "echo '$HOME'");
        let argv = vec!["echo".to_string(), "a\"b`c".to_string()];
        assert_eq!(escape_argv(&argv), "echo 'a\"b`c'");
    }

    #[test]
    fn empty_word_and_single_quote_escape() {
        let argv = vec!["echo".to_string(), String::new()];
        assert_eq!(escape_argv(&argv), "echo ''");
        let argv = vec!["echo".to_string(), "it's".to_string()];
        assert_eq!(escape_argv(&argv), "echo 'it'\\''s'");
    }

    #[test]
    fn tilde_quotes_to_stay_literal() {
        let argv = vec!["echo".to_string(), "~/.cargo/bin".to_string()];
        assert_eq!(escape_argv(&argv), "echo '~/.cargo/bin'");
    }
}
