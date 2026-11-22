//! Live progress bars.

use std::collections::BTreeMap;
use std::time::Duration;

use confit_events::protocol::TaskKind;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::shared::state::JobProgress;

/// One bar row for a live job.
struct Bar {
    /// The progress widget.
    bar: ProgressBar,
    /// Metered bar state.
    metered: bool,
}

/// Live progress bars.
pub struct Bars {
    /// The bar widget set.
    multi: MultiProgress,
    /// One bar row per live job.
    bars: BTreeMap<TaskKind, Bar>,
}

impl Bars {
    /// Builds live widgets.
    pub fn new() -> Self {
        Self {
            multi: MultiProgress::new(),
            bars: BTreeMap::new(),
        }
    }

    /// Prints one persisted line.
    pub fn print(&self, line: &str) {
        self.suspend(|| eprintln!("{line}"));
    }

    /// Suspends the widgets and runs the closure.
    pub fn suspend<R>(&self, run: impl FnOnce() -> R) -> R {
        self.multi.suspend(run)
    }

    /// Clears every bar row.
    pub fn clear(&mut self) {
        for bar in self.bars.values() {
            bar.bar.finish_and_clear();
        }
        self.bars.clear();
    }

    /// Opens one job row.
    pub fn started(&mut self, kind: &TaskKind, label: &str) {
        sync_spinner(self, kind, label.to_owned());
    }

    /// Advances one job row and changes spinners to meters.
    pub fn progressed(&mut self, kind: &TaskKind, label: &str, progress: JobProgress) {
        match progress.total {
            Some(known) => {
                sync_metered(self, kind, label.to_owned(), progress.done, known);
            }
            None => sync_spinner(self, kind, label.to_owned()),
        }
    }

    /// Closes one job row and drops its bar.
    pub fn finished(&mut self, kind: &TaskKind) {
        if let Some(bar) = self.bars.remove(kind) {
            bar.bar.finish_and_clear();
        }
    }
}

/// Syncs one metered row.
fn sync_metered(bars: &mut Bars, kind: &TaskKind, message: String, done: usize, total: usize) {
    let bar = bars.bars.entry(kind.clone()).or_insert_with(|| Bar {
        bar: spinner(&bars.multi),
        metered: false,
    });
    if !bar.metered {
        bar.bar.finish_and_clear();
        bar.bar = metered_bar(&bars.multi, total as u64);
        bar.metered = true;
    }
    bar.bar.set_style(bar_style_files());
    bar.bar.set_length(total as u64);
    bar.bar.set_message(message);
    bar.bar.set_position(done as u64);
}

/// Syncs one spinner row.
fn sync_spinner(bars: &mut Bars, kind: &TaskKind, message: String) {
    let bar = bars.bars.entry(kind.clone()).or_insert_with(|| Bar {
        bar: spinner(&bars.multi),
        metered: false,
    });
    if bar.metered {
        bar.bar.finish_and_clear();
        bar.bar = spinner(&bars.multi);
        bar.metered = false;
    }
    bar.bar.set_style(spinner_style());
    bar.bar.set_message(message);
    bar.bar.tick();
}

/// Builds one spinner row.
fn spinner(multi: &MultiProgress) -> ProgressBar {
    let bar = multi.add(ProgressBar::new_spinner());
    bar.set_style(spinner_style());
    bar.enable_steady_tick(Duration::from_millis(80));
    bar
}

/// Builds one metered row.
fn metered_bar(multi: &MultiProgress, total: u64) -> ProgressBar {
    let bar = multi.add(ProgressBar::new(total));
    bar.enable_steady_tick(Duration::from_millis(80));
    bar
}

/// Builds the spinner style.
fn spinner_style() -> ProgressStyle {
    match ProgressStyle::with_template("{spinner:.green} {msg}") {
        Ok(style) => style.tick_chars("|/-\\ "),
        Err(_) => ProgressStyle::default_spinner(),
    }
}

/// Builds the file bar style.
fn bar_style_files() -> ProgressStyle {
    let template = "{spinner:.green} {msg} {bar:40.cyan/blue} \
        {pos}/{len} ({per_sec}, {eta})";
    match ProgressStyle::with_template(template) {
        Ok(style) => style.progress_chars("#>-"),
        Err(_) => ProgressStyle::default_bar(),
    }
}
