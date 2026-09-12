//! Presentation
//!
//! Every string the user sees with color. User facing text stays apart
//! from the logic that computes what changed.

use std::borrow::Cow;

use crate::error::Result;
use crate::model::dto::diff::ArtifactDetail;
use crate::model::dto::diff::ArtifactStatus;
use crate::model::dto::diff::ChangeKind;
use crate::model::dto::diff::ChangeLine;
use crate::model::dto::diff::DiskDetail;
use crate::model::dto::diff::EntryChange;
use crate::model::dto::diff::PlanSummary;
use crate::model::dto::diff::Sigil;
use crate::model::dto::outcome::PlanOutcome;
use crate::model::dto::outcome::StatusOutcome;
use crate::model::dto::warning::PlanWarning;
use crate::model::dto::warning::WarningKind;
use crate::model::state::artifact::Artifact;
use crate::model::state::artifact::ArtifactData;
use crate::model::state::artifact::ArtifactKind;
use crate::model::state::plan::Plan;
use crate::model::state::rc::EnvEntry;
use crate::model::state::rc::RcData;

/// Palette: one ANSI style per change sigil.
const UPDATE_STYLE: &str = "\x1b[33m";
const ADD_STYLE: &str = "\x1b[32m";
const REMOVE_STYLE: &str = "\x1b[31m";
const HEADER_STYLE: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

/// Maps a sigil to its body mark.
///
/// # Arguments
///
/// * `sigil` - the sigil under display.
///
/// # Returns
///
/// The body mark for the sigil, holding `None` for context and header sigils.
pub fn sigil_mark(sigil: Sigil) -> Option<char> {
    match sigil {
        Sigil::Add => Some('+'),
        Sigil::Remove => Some('-'),
        Sigil::Update => Some('~'),
        Sigil::Context | Sigil::Header => None,
    }
}

/// Paints an owned line in the sigil style.
///
/// # Arguments
///
/// * `sigil` - the style selecting the line color.
/// * `text` - the line body under paint.
///
/// # Returns
///
/// The painted line, ready for terminal display.
pub fn paint_owned<'a>(sigil: Sigil, text: String) -> Cow<'a, str> {
    if !color_on() {
        return Cow::Owned(text);
    }
    match sigil {
        Sigil::Update => Cow::Owned(format!("{UPDATE_STYLE}{text}{RESET}")),
        Sigil::Add => Cow::Owned(format!("{ADD_STYLE}{text}{RESET}")),
        Sigil::Remove => Cow::Owned(format!("{REMOVE_STYLE}{text}{RESET}")),
        Sigil::Header => Cow::Owned(format!("{HEADER_STYLE}{text}{RESET}")),
        Sigil::Context => Cow::Owned(text),
    }
}
/// Paints a borrowed line in the sigil style.
///
/// # Arguments
///
/// * `sigil` - the style selecting the line color.
/// * `text` - the line body under paint.
///
/// # Returns
///
/// The painted line, ready for terminal display.
pub fn paint(sigil: Sigil, text: &str) -> Cow<'_, str> {
    if !color_on() {
        return Cow::Borrowed(text);
    }
    match sigil {
        Sigil::Update => Cow::Owned(format!("{UPDATE_STYLE}{text}{RESET}")),
        Sigil::Add => Cow::Owned(format!("{ADD_STYLE}{text}{RESET}")),
        Sigil::Remove => Cow::Owned(format!("{REMOVE_STYLE}{text}{RESET}")),
        Sigil::Header => Cow::Owned(format!("{HEADER_STYLE}{text}{RESET}")),
        Sigil::Context => Cow::Borrowed(text),
    }
}

/// Reports whether terminal color applies.
///
/// # Returns
///
/// `true` while color output applies.
fn color_on() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::IsTerminal::is_terminal(&std::io::stderr())
}

/// Renders one filesystem warning as one line.
///
/// # Arguments
///
/// * `warning` - the warning under display.
///
/// # Returns
///
/// The warning line for terminal display.
pub fn warning_line(warning: &PlanWarning) -> String {
    match &warning.kind {
        WarningKind::OverwriteUntracked => {
            format!(
                "{}: exists but no record: will be overwritten",
                warning.path
            )
        }
        WarningKind::ManualModification => {
            format!(
                "{}: differs from recorded: manual modification will be overwritten",
                warning.path
            )
        }
        WarningKind::Unreadable { reason } => reason.clone(),
    }
}

/// Renders one structured change item as one plain line.
///
/// # Arguments
///
/// * `kind` - the artifact kind holding the line.
/// * `line` - the change line under display.
///
/// # Returns
///
/// The plain line for terminal display.
pub fn render_change_line(kind: ArtifactKind, line: &ChangeLine) -> String {
    if line.is_header() {
        return line.key.clone();
    }
    match (&line.old, &line.new) {
        (Some(old), Some(new)) => {
            format!("  ~ {} = {old} -> {new} (will be overwritten)", line.key)
        }
        (None, Some(new)) => format!("  + {} = {new}", line.key),
        (Some(old), None) => format!("  - {} = {old}", line.key),
        (None, None) => render_text_line(kind, line),
    }
}

/// Renders one text-only change line.
///
/// # Arguments
///
/// * `kind` - the artifact kind holding the line.
/// * `line` - the change line under display.
///
/// # Returns
///
/// The plain line for terminal display.
fn render_text_line(kind: ArtifactKind, line: &ChangeLine) -> String {
    match line.sigil {
        Sigil::Context => format!(" {}", line.key),
        Sigil::Add | Sigil::Remove => {
            let mark = sigil_mark(line.sigil).unwrap_or('+');
            if kind == ArtifactKind::Rc {
                format!("  {mark} {}", line.key)
            } else {
                format!("{mark}{}", line.key)
            }
        }
        Sigil::Update => format!("  ~ {}", line.key),
        Sigil::Header => line.key.clone(),
    }
}

/// Paints one rendered change line.
///
/// # Arguments
///
/// * `kind` - the artifact kind holding the line.
/// * `line` - the change line under paint.
///
/// # Returns
///
/// The painted line, ready for terminal display.
pub fn paint_change_line(kind: ArtifactKind, line: &ChangeLine) -> String {
    paint_owned(line.sigil, render_change_line(kind, line)).into_owned()
}

/// Renders the drift note.
///
/// # Arguments
///
/// * `plan` - the desired plan under comparison.
/// * `disk` - the disk snapshots under comparison.
///
/// # Returns
///
/// The drift note, holding an empty string while disk aligns with the baseline.
pub fn render_drift(plan: &Plan, disk: &[DiskDetail]) -> String {
    let mut blocks = Vec::new();
    for artifact in &plan.artifacts {
        let key = format!("{}:{}", artifact.kind, artifact.path);
        let detail = disk.iter().find(|entry| entry.key == key);
        let Some(detail) = detail else {
            continue;
        };
        if detail.lines.is_empty() {
            continue;
        }
        let mut block: Vec<Cow<'_, str>> = vec![
            paint_owned(Sigil::Header, format!("  # {} has changed", artifact.path)),
            paint_owned(
                Sigil::Header,
                format!(
                    "  ~ artifact \"{}\" \"{}\" {{",
                    artifact.kind, artifact.path
                ),
            ),
        ];
        for line in &detail.lines {
            let plain = format!("  {}", render_change_line(artifact.kind, line));
            block.push(paint_owned(line.sigil, plain));
        }
        block.push(Cow::Borrowed(
            "        # (all other unchanged attributes hidden)",
        ));
        block.push(Cow::Borrowed("    }"));
        blocks.push(block.join("\n"));
    }
    if blocks.is_empty() {
        return String::new();
    }
    format!(
        "Note: Objects have changed outside of ConfIt\n\nConfIt detected the following changes made outside of ConfIt since the last recorded state:\n\n{}\n\nUnless you have made equivalent changes to your configuration, the plan below may include actions to undo or respond to these changes.",
        blocks.join("\n\n")
    )
}

/// Renders the plan summary.
///
/// # Arguments
///
/// * `plan` - the desired plan under display.
/// * `summary` - the counts backing the closing line.
/// * `details` - the per-artifact diffs backing the body.
/// * `show_conflicts` - the flag selecting winner attribution on changed lines.
///
/// # Returns
///
/// The plan text, closing with the add, change, and destroy counts.
pub fn render_plan(
    plan: &Plan,
    summary: &PlanSummary,
    details: &[ArtifactDetail],
    show_conflicts: bool,
) -> String {
    let mut lines: Vec<Cow<'_, str>> = Vec::new();
    for artifact in &plan.artifacts {
        let key = format!("{}:{}", artifact.kind, artifact.path);
        let detail = details.iter().find(|detail| detail.key == key);
        let status = detail.map(|detail| detail.status);
        lines.push(paint_owned(Sigil::Header, header_line(artifact, status)));
        let Some(detail) = detail else {
            continue;
        };
        if detail.status == ArtifactStatus::Unchanged {
            continue;
        }
        let body = if let ArtifactData::Rc(rc) = &artifact.data {
            rc_entry_lines(rc, &detail.entries, show_conflicts)
        } else {
            detail
                .entries
                .iter()
                .map(|entry| render_entry(entry, show_conflicts))
                .collect()
        };
        lines.extend(body.into_iter().map(Cow::Owned));
    }
    lines.push(paint_owned(
        Sigil::Header,
        format!(
            "Plan: {} to add, {} to change, {} to destroy.",
            summary.create, summary.update, summary.delete
        ),
    ));
    lines.join("\n")
}

/// Joins the drift note plus the plan summary.
///
/// # Arguments
///
/// * `drift` - the drift note, holding an empty string while disk aligns with the baseline.
/// * `plan_text` - the plan summary body.
///
/// # Returns
///
/// The full display text, holding the plan body alone for empty drift.
pub fn render_full(drift: &str, plan_text: &str) -> String {
    if drift.is_empty() {
        return plan_text.to_string();
    }
    format!("{drift}\n\n{plan_text}")
}

/// Renders the plan outcome for display.
///
/// # Arguments
///
/// * `outcome` - the plan outcome under display.
///
/// # Returns
///
/// The drift note joined with the plan summary.
pub fn render_plan_outcome(outcome: &PlanOutcome) -> String {
    let drift = render_drift(&outcome.plan, &outcome.disk);
    let body = render_plan(
        &outcome.plan,
        &outcome.summary,
        &outcome.details,
        outcome.conflicts,
    );
    render_full(&drift, &body)
}

/// Renders the status outcome for display.
///
/// # Arguments
///
/// * `outcome` - the status outcome under display.
///
/// # Returns
///
/// The drift note joined with the plan summary.
pub fn render_status_outcome(outcome: &StatusOutcome) -> String {
    let drift = render_drift(&outcome.plan, &outcome.disk);
    let body = render_plan(
        &outcome.plan,
        &outcome.summary,
        &outcome.details,
        outcome.conflicts,
    );
    render_full(&drift, &body)
}

/// Renders warning lines in outcome order.
///
/// # Arguments
///
/// * `warnings` - the warnings under display.
///
/// # Returns
///
/// The warning lines, one per outcome entry.
pub fn render_warnings(warnings: &[PlanWarning]) -> Vec<String> {
    warnings.iter().map(warning_line).collect()
}

/// Renders the plan payload for export.
///
/// # Arguments
///
/// * `plan` - the plan under serialization.
///
/// # Returns
///
/// The serialized plan payload.
///
/// # Errors
///
/// Failure serializing the plan payload.
pub fn render_plan_payload(plan: &Plan) -> Result<String> {
    crate::services::plan::serialize(plan)
}

/// Builds the header line for one artifact.
///
/// # Arguments
///
/// * `artifact` - the artifact naming the header.
/// * `status` - the lifecycle status marking updates.
///
/// # Returns
///
/// The header line, listing contributing tools where present.
fn header_line(artifact: &Artifact, status: Option<ArtifactStatus>) -> String {
    let mut tools: Vec<(&u64, &str)> = artifact
        .contributions
        .iter()
        .map(|contribution| (&contribution.order, contribution.tool.as_str()))
        .collect();
    tools.sort();
    let names: Vec<&str> = tools.into_iter().map(|(_, tool)| tool).collect();
    let mut header = format!("{}: {}", artifact.path, artifact.kind);
    if status == Some(ArtifactStatus::Update) {
        header.push_str(" ~ update");
    }
    if !names.is_empty() {
        header.push_str(&format!(" ← {}", names.join(", ")));
    }
    header
}

/// Maps an entry change to its display sigil.
///
/// # Arguments
///
/// * `change` - the entry change under display.
///
/// # Returns
///
/// The sigil marking the change shape.
fn entry_sigil(change: &ChangeKind) -> Sigil {
    match change {
        ChangeKind::Added { .. } => Sigil::Add,
        ChangeKind::Removed { .. } => Sigil::Remove,
        ChangeKind::Changed { .. } => Sigil::Update,
        ChangeKind::Unchanged { .. } => Sigil::Context,
    }
}

/// Renders one entry change as one painted line.
///
/// # Arguments
///
/// * `entry` - the entry change under display.
/// * `show_conflicts` - the flag selecting winner attribution on changed lines.
///
/// # Returns
///
/// The painted entry line.
fn render_entry(entry: &EntryChange, show_conflicts: bool) -> String {
    let plain = match &entry.change {
        ChangeKind::Added { value } => match &entry.tool {
            Some(tool) => format!("  + {} = {value} ({tool})", entry.label),
            None => format!("  + {} = {value}", entry.label),
        },
        ChangeKind::Changed { from, to } => {
            let suffix = match (&entry.tool, &entry.over) {
                (Some(tool), Some(over)) if show_conflicts => {
                    format!(" ({tool} wins over {over})")
                }
                (Some(tool), _) => format!(" ({tool})"),
                (None, _) => String::new(),
            };
            format!("  ~ {} = {from} → {to}{suffix}", entry.label)
        }
        ChangeKind::Removed { value } => {
            format!("  - {} = {value}", entry.label)
        }
        ChangeKind::Unchanged { value } => match &entry.tool {
            Some(tool) => format!("    {} = {value} ({tool})", entry.label),
            None => format!("    {} = {value}", entry.label),
        },
    };
    paint_owned(entry_sigil(&entry.change), plain).into_owned()
}

/// Orders rc entries for display.
///
/// # Arguments
///
/// * `rc` - the rc data backing env grouping.
/// * `entries` - the entry changes under order.
/// * `show_conflicts` - the flag selecting winner attribution on changed lines.
///
/// # Returns
///
/// The ordered display lines.
fn rc_entry_lines(rc: &RcData, entries: &[EntryChange], show_conflicts: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for entry in entries {
        if entry.label.starts_with("alias ") && !matches!(entry.change, ChangeKind::Removed { .. })
        {
            lines.push(render_entry(entry, show_conflicts));
        }
    }
    let env_entries: Vec<&EntryChange> = entries
        .iter()
        .filter(|entry| {
            is_env_label(&entry.label) && !matches!(entry.change, ChangeKind::Removed { .. })
        })
        .collect();
    for (name, value, conditional) in env_winners(rc.env.iter()) {
        let group: Vec<&&EntryChange> = env_entries
            .iter()
            .filter(|entry| entry.label == name)
            .collect();
        if group.is_empty() {
            continue;
        }
        lines.push(render_grouped_env(
            name,
            value,
            conditional,
            &group,
            show_conflicts,
        ));
    }
    for entry in entries {
        if is_env_label(&entry.label) && matches!(entry.change, ChangeKind::Removed { .. }) {
            lines.push(render_entry(entry, show_conflicts));
        }
    }
    for entry in entries {
        if (entry.label.starts_with("profile ") || entry.label.starts_with("init["))
            && !matches!(entry.change, ChangeKind::Removed { .. })
        {
            lines.push(render_entry(entry, show_conflicts));
        }
    }
    for entry in entries {
        if matches!(entry.change, ChangeKind::Removed { .. }) && !is_env_label(&entry.label) {
            lines.push(render_entry(entry, show_conflicts));
        }
    }
    lines
}

/// Reports whether a label names a plain env entry.
///
/// # Arguments
///
/// * `label` - the entry label under test.
///
/// # Returns
///
/// `true` for plain env names.
fn is_env_label(label: &str) -> bool {
    !label.starts_with("alias ")
        && !label.starts_with("profile ")
        && !label.starts_with("init[")
        && !label.starts_with("vars.")
        && *label != *"src"
        && *label != *"content"
        && *label != *"target"
        && *label != *"source"
        && !label.contains('.')
        && !label.contains('[')
}

/// Renders one env name as a single grouped line.
///
/// # Arguments
///
/// * `name` - the env name under display.
/// * `value` - the winning value under display.
/// * `conditional` - the count of conditional overrides for the name.
/// * `group` - the entry changes sharing the name.
/// * `show_conflicts` - the flag selecting winner attribution on changed lines.
///
/// # Returns
///
/// The painted grouped line.
fn render_grouped_env(
    name: &str,
    value: &str,
    conditional: usize,
    group: &[&&EntryChange],
    show_conflicts: bool,
) -> String {
    let overrides = if conditional == 0 {
        String::new()
    } else {
        format!(" (+ {conditional} conditional overrides)")
    };
    let changed = group.iter().find_map(|entry| match &entry.change {
        ChangeKind::Changed { from, to } => Some((
            from.clone(),
            to.clone(),
            entry.tool.clone(),
            entry.over.clone(),
        )),
        _ => None,
    });
    let is_changed = changed.is_some();
    let plain = if let Some((from, to, tool, over)) = changed {
        let suffix = match (&tool, &over) {
            (Some(winner), Some(loser)) if show_conflicts => {
                format!("{overrides} ({winner} wins over {loser})")
            }
            (Some(winner), _) => format!("{overrides} ({winner})"),
            (None, _) => overrides,
        };
        format!("  ~ {name} = {from} → {to}{suffix}")
    } else if let Some(added) = group
        .iter()
        .find(|entry| matches!(entry.change, ChangeKind::Added { .. }))
    {
        let suffix = match &added.tool {
            Some(tool) => format!("{overrides} ({tool})"),
            None => overrides,
        };
        format!("  + {name} = \"{value}\"{suffix}")
    } else {
        let tool = group
            .iter()
            .find_map(|entry| match &entry.change {
                ChangeKind::Unchanged { .. } => entry.tool.clone(),
                _ => None,
            })
            .or_else(|| group.first().and_then(|entry| entry.tool.clone()));
        let suffix = match &tool {
            Some(tool) => format!("{overrides} ({tool})"),
            None => overrides,
        };
        format!("    {name} = \"{value}\"{suffix}")
    };
    let sigil = if is_changed {
        Sigil::Update
    } else if group
        .iter()
        .any(|entry| matches!(entry.change, ChangeKind::Added { .. }))
    {
        Sigil::Add
    } else {
        Sigil::Context
    };
    paint_owned(sigil, plain).into_owned()
}

/// Groups env entries by name for display.
///
/// # Arguments
///
/// * `entries` - the env entries under grouping.
///
/// # Returns
///
/// The name, winning value, and conditional count per group, in first-seen order.
fn env_winners<'a>(entries: impl Iterator<Item = &'a EnvEntry>) -> Vec<(&'a str, &'a str, usize)> {
    let mut groups: Vec<(&'a str, Vec<&'a EnvEntry>)> = Vec::new();
    for entry in entries {
        match groups.iter_mut().find(|(name, _)| *name == entry.name) {
            Some((_, members)) => members.push(entry),
            None => groups.push((entry.name.as_str(), vec![entry])),
        }
    }
    groups
        .into_iter()
        .map(|(name, members)| {
            let conditional = members.iter().filter(|entry| entry.when.is_some()).count();
            let display = members
                .iter()
                .find(|entry| entry.when.is_none())
                .or(members.first())
                .map(|entry| entry.value.as_str())
                .unwrap_or("");
            (name, display, conditional)
        })
        .collect()
}
