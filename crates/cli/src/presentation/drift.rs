//! Drift lines
//!
//! Display lines over core drift entries.

use confit_core::drift::Drift;

/// Renders drift entries as display lines.
///
/// One or more display lines per drift entry.
///
/// # Examples
///
/// ```rust
/// use confit_cli::presentation::drift::drift_lines;
/// use confit_core::drift::Drift;
/// use confit_core::handles::{Route, RouteBase};
///
/// let entries = vec![
///     Drift::Key {
///         path: Route::new(RouteBase::Home, "app.toml").unwrap(),
///         key: "tools.bat".to_string(),
///         old: Some(serde_json::json!("old")),
///         new: Some(serde_json::json!("new")),
///     },
///     Drift::Missing { path: Route::new(RouteBase::Home, "note").unwrap() },
/// ];
/// assert_eq!(
///     drift_lines(&entries),
///     vec![
///         "~ home:app.toml: tools.bat = old -> new".to_string(),
///         "home:note: manually deleted. changed outside config: add to config or the next apply loses them"
///             .to_string(),
///     ]
/// );
/// ```
pub fn drift_lines(entries: &[Drift]) -> Vec<String> {
    let mut lines = Vec::new();
    for entry in entries {
        match entry {
            Drift::Key {
                path,
                key,
                old,
                new,
            } => match (old, new) {
                (Some(old_value), Some(new_value)) => {
                    lines.push(format!(
                        "~ {}: {} = {} -> {}",
                        path.display(),
                        key,
                        leaf_text(old_value),
                        leaf_text(new_value)
                    ));
                }
                (Some(old_value), None) => {
                    lines.push(format!(
                        "- {}: {} = {}",
                        path.display(),
                        key,
                        leaf_text(old_value)
                    ));
                }
                (None, Some(new_value)) => {
                    lines.push(format!(
                        "+ {}: {} = {}",
                        path.display(),
                        key,
                        leaf_text(new_value)
                    ));
                }
                (None, None) => {}
            },
            Drift::Hunk { hunks, .. } => {
                for line in hunks.lines() {
                    if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@")
                    {
                        continue;
                    }
                    lines.push(line.to_string());
                }
            }
            Drift::Missing { path } => {
                lines.push(format!(
                    "{}: manually deleted. changed outside config: add to config or the next apply loses them",
                    path.display()
                ));
            }
            Drift::Unreadable { path, reason } => {
                lines.push(format!(
                    "cannot read '{}': {reason}. changed outside config: add to config or the next apply loses them",
                    path.display()
                ));
            }
        }
    }
    lines
}

/// Renders one scalar leaf value as display text.
fn leaf_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Number(_) | serde_json::Value::Bool(_) => value.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
        }
    }
}
