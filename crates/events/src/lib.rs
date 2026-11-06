//! Workers send progress, plans, prints, and prompts to the render thread.

mod protocol;

use indicatif::MultiProgress;
pub use protocol::{ApplyOutcome, Change, ChangeSource, FromUi, Plan, PlanAction, PlanEntry, ToUi};

use std::{collections::HashMap, io, time::Duration};

use confit_model::routes::Route;
use crossbeam_channel::{Receiver, Sender};

/// The channel ends that connect worker threads to the render thread.
#[derive(Debug, Clone)]
pub struct EventHandles {
    /// The end that sends progress, plans, prints, and
    /// prompts to the render thread.
    pub to_ui: Sender<ToUi>,
    /// The end that receives answers and confirmations
    /// from the render thread.
    pub from_ui: Receiver<FromUi>,
}

impl EventHandles {
    /// Sends one question to the render thread and returns its answer.
    ///
    /// - Callers hold at most one pending prompt at a time.
    /// - A dismissal maps to the closed channel error.
    ///
    /// # Errors
    ///
    /// - [`std::io::Error`] when a channel closes.
    /// - [`std::io::Error`] when a question gets an approval.
    /// - [`std::io::Error`] when the render thread dismisses the prompt.
    pub fn ask(&self, prompt: String) -> io::Result<String> {
        if self.to_ui.send(ToUi::Prompt(prompt)).is_err() {
            return Err(send_err_closed());
        }
        match self.from_ui.recv() {
            Ok(FromUi::Answer(answer)) => Ok(answer),
            Ok(FromUi::Approved) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ui answered with a confirmation",
            )),
            Ok(FromUi::Close) => Err(send_err_closed()),
            Err(_) => Err(send_err_closed()),
        }
    }

    /// Sends one confirmation request to the render thread.
    ///
    /// - Callers hold at most one pending prompt at a time.
    /// - Approval resolves `Ok`, anything else resolves `Err`.
    ///
    /// # Errors
    ///
    /// - [`std::io::Error`] when a channel closes.
    /// - [`std::io::Error`] when a confirmation gets a text answer.
    /// - [`std::io::Error`] when the render thread dismisses the request.
    pub fn confirm(&self) -> io::Result<()> {
        if self.to_ui.send(ToUi::Confirm).is_err() {
            return Err(send_err_closed());
        }
        match self.from_ui.recv() {
            Ok(FromUi::Approved) => Ok(()),
            Ok(FromUi::Answer(_)) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ui answered with text",
            )),
            Ok(FromUi::Close) => Err(send_err_closed()),
            Err(_) => Err(send_err_closed()),
        }
    }

    /// Builds memory channels with no render thread.
    ///
    /// - Prompt calls fail with a closed channel error.
    pub fn silent() -> Self {
        let (to_ui, _) = crossbeam_channel::unbounded::<ToUi>();
        let (_, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        Self { to_ui, from_ui }
    }
}

/// The running renderer.
pub struct Ui {
    /// The channels that reach the render thread.
    handles: EventHandles,
    /// The join handle for the render thread, used at teardown.
    renderer: Option<std::thread::JoinHandle<()>>,
}

impl Ui {
    /// Starts the render thread with unbounded worker channels.
    pub fn start() -> Self {
        let (to_ui_tx, to_ui_rx) = crossbeam_channel::unbounded::<ToUi>();
        let (answer_tx, answer_rx) = crossbeam_channel::unbounded::<FromUi>();
        let renderer = std::thread::Builder::new()
            .name("confit-progress".to_owned())
            .spawn(move || run_loop(to_ui_rx, answer_tx))
            .ok();
        Self {
            handles: EventHandles {
                to_ui: to_ui_tx,
                from_ui: answer_rx,
            },
            renderer,
        }
    }

    /// Returns a clone of the worker channels.
    pub fn handles(&self) -> EventHandles {
        self.handles.clone()
    }

    /// Drops the worker channels and waits for the render thread to stop.
    pub fn finish(self) {
        let Self { handles, renderer } = self;
        drop(handles);
        if let Some(renderer) = renderer {
            let _ = renderer.join();
        }
    }
}

/// One widget row with its last measured counts.
struct Widget {
    /// The spinner or bar on the row.
    bar: indicatif::ProgressBar,
    /// The last measured update, absent for spinners.
    metered: Option<(usize, usize)>,
}

/// The key for one widget row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum WidgetKey {
    /// One download URL.
    Fetch(String),
    /// The pool fill.
    PoolFill,
    /// The pool prune.
    PoolPrune,
    /// One archive path.
    Archive(std::path::PathBuf),
    /// The apply run.
    Apply,
    /// One hook run.
    Hook(String),
}

/// Renders progress, prints, and prompts until every channel closes.
fn run_loop(rx: Receiver<ToUi>, answers: Sender<FromUi>) {
    use std::io::IsTerminal as _;
    let terminal = io::stderr().is_terminal();
    let multi = if terminal {
        MultiProgress::new()
    } else {
        MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden())
    };
    let mut bars: HashMap<WidgetKey, Widget> = HashMap::new();
    loop {
        let message = rx.recv();
        if let Ok(event) = message.as_ref() {
            log::debug!("event: {event:?}");
        }
        match message {
            Ok(ToUi::FetchStarted { url }) => {
                let widget = bars
                    .entry(WidgetKey::Fetch(url.clone()))
                    .or_insert_with(|| Widget {
                        bar: spinner(&multi),
                        metered: None,
                    });
                widget.bar.set_style(spinner_style());
                widget
                    .bar
                    .set_message(format!("collecting artifacts: {url}"));
                widget.bar.tick();
            }
            Ok(ToUi::FetchProgress { url, done, total }) => {
                let widget = bars
                    .entry(WidgetKey::Fetch(url.clone()))
                    .or_insert_with(|| Widget {
                        bar: spinner(&multi),
                        metered: None,
                    });
                match total {
                    Some(total) => {
                        if widget.metered.is_none() {
                            let bar = multi.add(indicatif::ProgressBar::new(total as u64));
                            bar.enable_steady_tick(Duration::from_millis(80));
                            widget.bar = bar;
                        }
                        widget.bar.set_style(bar_style_bytes());
                        widget.bar.set_length(total as u64);
                        widget
                            .bar
                            .set_message(format!("collecting artifacts: {url}"));
                        widget.bar.set_position(done as u64);
                        widget.metered = Some((done, total));
                    }
                    None => {
                        widget.bar.set_style(spinner_style());
                        widget
                            .bar
                            .set_message(format!("collecting artifacts: {url}"));
                        widget.bar.tick();
                    }
                }
            }
            Ok(ToUi::FetchFinished { url, cached }) => {
                let metered = bars
                    .get(&WidgetKey::Fetch(url.clone()))
                    .and_then(|widget| widget.metered);
                let line = match (cached, metered) {
                    (true, _) => format!("collecting artifacts: {url} (cached)"),
                    (false, Some((done, total))) => format!(
                        "collecting artifacts: {url} ({} of {})",
                        human_bytes(done),
                        human_bytes(total)
                    ),
                    (false, None) => format!("collecting artifacts: {url}"),
                };
                let _ = multi.println(line);
                bars.remove(&WidgetKey::Fetch(url));
            }
            Ok(ToUi::PoolFillStarted { total }) => {
                start_bar(&mut bars, &multi, WidgetKey::PoolFill, total);
                if let Some(widget) = bars.get(&WidgetKey::PoolFill) {
                    widget.bar.set_message("compressing pool entries");
                }
            }
            Ok(ToUi::PoolFillProgress { done, total }) => {
                advance_bar(&mut bars, &multi, WidgetKey::PoolFill, done, total);
                if let Some(widget) = bars.get(&WidgetKey::PoolFill) {
                    widget.bar.set_message("compressing pool entries");
                }
            }
            Ok(ToUi::PoolFillFinished) => {
                finish_bar(
                    &mut bars,
                    &multi,
                    &WidgetKey::PoolFill,
                    "compressing pool entries",
                );
            }
            Ok(ToUi::PoolPruneStarted { total }) => {
                start_bar(&mut bars, &multi, WidgetKey::PoolPrune, total);
                if let Some(widget) = bars.get(&WidgetKey::PoolPrune) {
                    widget.bar.set_message("pruning pool entries");
                }
            }
            Ok(ToUi::PoolPruneProgress { done, total }) => {
                advance_bar(&mut bars, &multi, WidgetKey::PoolPrune, done, total);
                if let Some(widget) = bars.get(&WidgetKey::PoolPrune) {
                    widget.bar.set_message("pruning pool entries");
                }
            }
            Ok(ToUi::PoolPruneFinished) => {
                finish_bar(
                    &mut bars,
                    &multi,
                    &WidgetKey::PoolPrune,
                    "pruning pool entries",
                );
            }
            Ok(ToUi::ArchiveStarted { path }) => {
                let widget = bars
                    .entry(WidgetKey::Archive(path.clone()))
                    .or_insert_with(|| Widget {
                        bar: spinner(&multi),
                        metered: None,
                    });
                widget.bar.set_style(spinner_style());
                widget
                    .bar
                    .set_message(format!("collecting artifacts: {}", path.display()));
                widget.bar.tick();
            }
            Ok(ToUi::ArchiveFinished { path, members }) => {
                let _ = multi.println(format!(
                    "collecting artifacts: {} ({members} members)",
                    path.display()
                ));
                bars.remove(&WidgetKey::Archive(path));
            }
            Ok(ToUi::ApplyStarted { total }) => {
                start_apply(&mut bars, &multi, total);
            }
            Ok(ToUi::ApplyProgress { route, outcome }) => {
                advance_apply(&mut bars, &multi, &route, &outcome);
            }
            Ok(ToUi::ApplyFinished) => {
                finish_apply(&mut bars, &multi);
            }
            Ok(ToUi::HookStarted {
                position,
                total,
                argv,
            }) => {
                let widget = bars
                    .entry(WidgetKey::Hook(argv.clone()))
                    .or_insert_with(|| Widget {
                        bar: spinner(&multi),
                        metered: None,
                    });
                widget.bar.set_style(spinner_style());
                widget
                    .bar
                    .set_message(format!("hook {position} of {total}: {argv}"));
                widget.bar.tick();
            }
            Ok(ToUi::HookFinished {
                position,
                total,
                argv,
            }) => {
                let _ = multi.println(format!("hook {position} of {total} done: {argv}"));
                bars.remove(&WidgetKey::Hook(argv));
            }
            Ok(ToUi::Plan(_)) => {
                log::debug!("plan received");
            }
            Ok(ToUi::Print(line)) => {
                let _ = multi.println(line);
            }
            Ok(ToUi::Prompt(question)) => respond_text(&multi, &answers, &question),
            Ok(ToUi::Confirm) => respond_confirm(&multi, &answers),
            Err(_) => break,
        }
    }
    for widget in bars.values() {
        widget.bar.finish_and_clear();
    }
}

/// Builds the broken pipe error that a closed ui channel reports.
fn send_err_closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "ui closed")
}

/// Parks the widgets, reads one stdin line, and sends the answer back.
fn respond_text(multi: &MultiProgress, answers: &Sender<FromUi>, question: &str) {
    match multi.suspend(|| read_answer(question)) {
        Ok(text) => {
            let _ = answers.send(FromUi::Answer(text));
        }
        Err(error) => {
            log::debug!("prompt read failed: {error}");
            let _ = answers.send(FromUi::Answer(String::new()));
        }
    }
}

/// Parks the widgets, reads one stdin line, and sends the approval back.
///
/// - A trimmed `yes` approves; anything else dismisses.
fn respond_confirm(multi: &MultiProgress, answers: &Sender<FromUi>) {
    const QUESTION: &str = "Apply these changes? Type 'yes' to continue: ";
    match multi.suspend(|| read_answer(QUESTION)) {
        Ok(text) => {
            let reply = if text.trim() == "yes" {
                FromUi::Approved
            } else {
                FromUi::Close
            };
            let _ = answers.send(reply);
        }
        Err(error) => {
            log::debug!("confirm read failed: {error}");
            let _ = answers.send(FromUi::Close);
        }
    }
}

/// Reads one stdin line and trims the trailing newline.
///
/// # Errors
///
/// - [`std::io::Error`] when stderr or stdin fails.
fn read_answer(question: &str) -> io::Result<String> {
    use std::io::{BufRead as _, Write as _};
    let mut stderr = io::stderr().lock();
    stderr.write_all(question.as_bytes())?;
    stderr.flush()?;
    log::debug!("prompt waiting for input");
    let mut line = String::new();
    let got = io::stdin().lock().read_line(&mut line)?;
    log::debug!("prompt read {got} bytes");
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

/// Starts one file bar for a batch operation.
fn start_bar(
    bars: &mut HashMap<WidgetKey, Widget>,
    multi: &MultiProgress,
    key: WidgetKey,
    total: usize,
) {
    let widget = bars.entry(key).or_insert_with(|| Widget {
        bar: {
            let bar = multi.add(indicatif::ProgressBar::new(total as u64));
            bar.enable_steady_tick(Duration::from_millis(80));
            bar
        },
        metered: None,
    });
    widget.bar.set_style(bar_style_files());
    widget.bar.set_length(total as u64);
}

/// Advances one file bar and records its counts.
fn advance_bar(
    bars: &mut HashMap<WidgetKey, Widget>,
    multi: &MultiProgress,
    key: WidgetKey,
    done: usize,
    total: usize,
) {
    start_bar(bars, multi, key.clone(), total);
    if let Some(widget) = bars.get_mut(&key) {
        widget.bar.set_position(done as u64);
        widget.metered = Some((done, total));
    }
}

/// Persists one file bar with its final counts and drops its widget.
fn finish_bar(
    bars: &mut HashMap<WidgetKey, Widget>,
    multi: &MultiProgress,
    key: &WidgetKey,
    label: &str,
) {
    let metered = bars.get(key).and_then(|widget| widget.metered);
    let line = match metered {
        Some((done, total)) => format!("{label} ({done} of {total} files)"),
        None => label.to_owned(),
    };
    let _ = multi.println(line);
    if let Some(widget) = bars.remove(key) {
        widget.bar.finish_and_clear();
    }
}

/// Opens the single apply bar at zero.
fn start_apply(bars: &mut HashMap<WidgetKey, Widget>, multi: &MultiProgress, total: usize) {
    let bar = multi.add(indicatif::ProgressBar::new(total as u64));
    bar.enable_steady_tick(Duration::from_millis(80));
    bar.set_style(bar_style_files());
    bar.set_message("applying");
    bar.set_position(0);
    bars.insert(
        WidgetKey::Apply,
        Widget {
            bar,
            metered: Some((0, total)),
        },
    );
}

/// Prints one outcome line and advances the apply bar by one.
///
/// - A tick without a preceding start falls back to a spinner row.
fn advance_apply(
    bars: &mut HashMap<WidgetKey, Widget>,
    multi: &MultiProgress,
    route: &Route,
    outcome: &ApplyOutcome,
) {
    let line = match outcome {
        ApplyOutcome::Added => format!("Added {}", route.display()),
        ApplyOutcome::Removed => format!("Removed {}", route.display()),
    };
    let _ = multi.println(line);
    match bars.get_mut(&WidgetKey::Apply) {
        Some(widget) => match widget.metered {
            Some((done, total)) => {
                let next = done + 1;
                widget.bar.set_position(next as u64);
                widget.metered = Some((next, total));
            }
            None => widget.bar.tick(),
        },
        None => {
            let widget = bars.entry(WidgetKey::Apply).or_insert_with(|| Widget {
                bar: spinner(multi),
                metered: None,
            });
            widget.bar.set_style(spinner_style());
            widget.bar.set_message("applying");
            widget.bar.tick();
        }
    }
}

/// Persists the apply counts and drops the apply widget.
fn finish_apply(bars: &mut HashMap<WidgetKey, Widget>, multi: &MultiProgress) {
    let metered = bars
        .get(&WidgetKey::Apply)
        .and_then(|widget| widget.metered);
    let line = match metered {
        Some((done, total)) => format!("{done} of {total} files applied"),
        None => "applying".to_owned(),
    };
    let _ = multi.println(line);
    if let Some(widget) = bars.remove(&WidgetKey::Apply) {
        widget.bar.finish_and_clear();
    }
}

/// Formats one byte count with a binary unit.
fn human_bytes(bytes: usize) -> String {
    const STEP: f64 = 1024.0;
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut scaled = bytes as f64;
    let mut unit = 0;
    while scaled >= STEP && unit + 1 < UNITS.len() {
        scaled /= STEP;
        unit += 1;
    }
    format!("{scaled:.1} {}", UNITS[unit])
}

/// Builds one spinner row on the widget set.
fn spinner(multi: &MultiProgress) -> indicatif::ProgressBar {
    let bar = multi.add(indicatif::ProgressBar::new_spinner());
    bar.set_style(spinner_style());
    bar.enable_steady_tick(Duration::from_millis(80));
    bar
}

/// Builds the spinner style, falling back to a plain spinner.
fn spinner_style() -> indicatif::ProgressStyle {
    match indicatif::ProgressStyle::with_template("{spinner:.green} {msg}") {
        Ok(style) => style.tick_chars("|/-\\ "),
        Err(_) => indicatif::ProgressStyle::default_spinner(),
    }
}

/// Builds the byte bar style, falling back to a plain bar.
fn bar_style_bytes() -> indicatif::ProgressStyle {
    let template =
        "{spinner:.green} {msg} {bar:40.cyan/blue} {bytes}/{total_bytes} ({bytes_per_sec}, {eta})";
    match indicatif::ProgressStyle::with_template(template) {
        Ok(style) => style.progress_chars("#>-"),
        Err(_) => indicatif::ProgressStyle::default_bar(),
    }
}

/// Builds the file bar style, falling back to a plain bar.
fn bar_style_files() -> indicatif::ProgressStyle {
    let template = "{spinner:.green} {msg} {bar:40.cyan/blue} {pos}/{len} ({per_sec}, {eta})";
    match indicatif::ProgressStyle::with_template(template) {
        Ok(style) => style.progress_chars("#>-"),
        Err(_) => indicatif::ProgressStyle::default_bar(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_finish_joins_the_renderer() {
        let ui = Ui::start();
        ui.finish();
    }

    #[test]
    fn ask_reports_a_closed_channel() {
        let (to_ui, rx) = crossbeam_channel::unbounded::<ToUi>();
        drop(rx);
        let (_, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        let handles = EventHandles { to_ui, from_ui };
        let error = handles.ask("proceed?".to_owned()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn run_loop_renders_progress_and_prints() {
        let (to_ui_tx, to_ui_rx) = crossbeam_channel::unbounded::<ToUi>();
        let (answer_tx, _answer_rx) = crossbeam_channel::unbounded::<FromUi>();
        to_ui_tx.send(ToUi::Print("hello".to_owned())).unwrap();
        to_ui_tx
            .send(ToUi::FetchStarted {
                url: "https://example.com/a".to_owned(),
            })
            .unwrap();
        to_ui_tx
            .send(ToUi::FetchProgress {
                url: "https://example.com/a".to_owned(),
                done: 1,
                total: Some(2),
            })
            .unwrap();
        to_ui_tx
            .send(ToUi::PoolFillProgress { done: 1, total: 2 })
            .unwrap();
        to_ui_tx
            .send(ToUi::FetchFinished {
                url: "https://example.com/a".to_owned(),
                cached: false,
            })
            .unwrap();
        drop(to_ui_tx);
        run_loop(to_ui_rx, answer_tx);
    }

    #[test]
    fn run_loop_covers_every_widget_operation() {
        use confit_model::routes::{Route, RouteBase};

        let (to_ui_tx, to_ui_rx) = crossbeam_channel::unbounded::<ToUi>();
        let (answer_tx, _answer_rx) = crossbeam_channel::unbounded::<FromUi>();
        let route = Route::new(RouteBase::Literal, "target/test-write.txt").unwrap();
        let sends = vec![
            ToUi::FetchStarted {
                url: "a".to_owned(),
            },
            ToUi::FetchProgress {
                url: "a".to_owned(),
                done: 1,
                total: None,
            },
            ToUi::FetchProgress {
                url: "a".to_owned(),
                done: 1,
                total: Some(2),
            },
            ToUi::FetchFinished {
                url: "a".to_owned(),
                cached: false,
            },
            ToUi::PoolFillStarted { total: 2 },
            ToUi::PoolFillProgress { done: 1, total: 2 },
            ToUi::PoolFillFinished,
            ToUi::PoolPruneStarted { total: 1 },
            ToUi::PoolPruneProgress { done: 1, total: 1 },
            ToUi::PoolPruneFinished,
            ToUi::ArchiveStarted {
                path: std::path::PathBuf::from("fonts.tar.gz"),
            },
            ToUi::ArchiveFinished {
                path: std::path::PathBuf::from("fonts.tar.gz"),
                members: 2,
            },
            ToUi::ApplyStarted { total: 2 },
            ToUi::ApplyProgress {
                route: route.clone(),
                outcome: ApplyOutcome::Added,
            },
            ToUi::ApplyProgress {
                route: route.clone(),
                outcome: ApplyOutcome::Removed,
            },
            ToUi::ApplyFinished,
            ToUi::HookStarted {
                position: 1,
                total: 1,
                argv: "tool run".to_owned(),
            },
            ToUi::HookFinished {
                position: 1,
                total: 1,
                argv: "tool run".to_owned(),
            },
            ToUi::Print("hello".to_owned()),
        ];
        for event in sends {
            to_ui_tx.send(event).unwrap();
        }
        drop(to_ui_tx);
        run_loop(to_ui_rx, answer_tx);
    }

    #[test]
    fn run_loop_finishes_files_with_final_counts() {
        let (to_ui_tx, to_ui_rx) = crossbeam_channel::unbounded::<ToUi>();
        let (answer_tx, _answer_rx) = crossbeam_channel::unbounded::<FromUi>();
        to_ui_tx
            .send(ToUi::PoolFillProgress { done: 1, total: 2 })
            .unwrap();
        to_ui_tx.send(ToUi::PoolFillFinished).unwrap();
        to_ui_tx
            .send(ToUi::PoolPruneProgress { done: 1, total: 1 })
            .unwrap();
        to_ui_tx.send(ToUi::PoolPruneFinished).unwrap();
        drop(to_ui_tx);
        run_loop(to_ui_rx, answer_tx);
    }

    #[test]
    fn apply_bar_counts_ticks_and_drops_on_finish() {
        use confit_model::routes::RouteBase;

        let multi = MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden());
        let mut bars: HashMap<WidgetKey, Widget> = HashMap::new();
        start_apply(&mut bars, &multi, 2);
        let widget = bars.get(&WidgetKey::Apply).unwrap();
        assert_eq!(widget.bar.position(), 0);
        assert_eq!(widget.metered, Some((0, 2)));
        let route = Route::new(RouteBase::Literal, "target/test-write.txt").unwrap();
        advance_apply(&mut bars, &multi, &route, &ApplyOutcome::Added);
        let widget = bars.get(&WidgetKey::Apply).unwrap();
        assert_eq!(widget.bar.position(), 1);
        assert_eq!(widget.metered, Some((1, 2)));
        advance_apply(&mut bars, &multi, &route, &ApplyOutcome::Removed);
        let widget = bars.get(&WidgetKey::Apply).unwrap();
        assert_eq!(widget.bar.position(), 2);
        assert_eq!(widget.metered, Some((2, 2)));
        finish_apply(&mut bars, &multi);
        assert!(!bars.contains_key(&WidgetKey::Apply));
    }

    #[test]
    fn apply_tick_without_a_start_falls_back_to_a_spinner() {
        use confit_model::routes::RouteBase;

        let multi = MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden());
        let mut bars: HashMap<WidgetKey, Widget> = HashMap::new();
        let route = Route::new(RouteBase::Literal, "target/test-write.txt").unwrap();
        advance_apply(&mut bars, &multi, &route, &ApplyOutcome::Added);
        let widget = bars.get(&WidgetKey::Apply).unwrap();
        assert_eq!(widget.metered, None);
        finish_apply(&mut bars, &multi);
        assert!(!bars.contains_key(&WidgetKey::Apply));
    }

    #[test]
    fn silent_handles_fail_prompts_without_a_thread() {
        let handles = EventHandles::silent();
        let ask = handles.ask("proceed?".to_owned()).unwrap_err();
        assert_eq!(ask.kind(), std::io::ErrorKind::BrokenPipe);
        let confirm = handles.confirm().unwrap_err();
        assert_eq!(confirm.kind(), std::io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn plan_and_confirm_travel_through_memory_channels() {
        let plan = Plan {
            entries: vec![PlanEntry {
                route: "home('.zshrc')".to_owned(),
                kind: "text".to_owned(),
                action: PlanAction::Change,
                changes: vec![Change::TextLine {
                    line: "export EDITOR=nvim".to_owned(),
                    source: ChangeSource::Desired,
                }],
                drift: true,
                rescue: true,
            }],
            warnings: vec!["one file changed on disk".to_owned()],
            first_run: false,
        };
        let (to_ui, to_ui_rx) = crossbeam_channel::unbounded::<ToUi>();
        to_ui.send(ToUi::Plan(plan.clone())).unwrap();
        assert_eq!(to_ui_rx.recv().unwrap(), ToUi::Plan(plan));
        let (confirm_tx, confirm_rx) = crossbeam_channel::unbounded::<FromUi>();
        confirm_tx.send(FromUi::Approved).unwrap();
        assert_eq!(confirm_rx.recv().unwrap(), FromUi::Approved);
    }

    #[test]
    fn confirm_ok_on_confirm_and_err_on_close() {
        let (to_ui, incoming) = crossbeam_channel::unbounded::<ToUi>();
        let (outgoing, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        let handles = EventHandles { to_ui, from_ui };
        outgoing.send(FromUi::Approved).unwrap();
        handles.confirm().unwrap();
        let sent = incoming.recv().unwrap();
        assert!(matches!(sent, ToUi::Confirm));
        outgoing.send(FromUi::Close).unwrap();
        let error = handles.confirm().unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn ask_err_on_close() {
        let (to_ui, _incoming) = crossbeam_channel::unbounded::<ToUi>();
        let (outgoing, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        let handles = EventHandles { to_ui, from_ui };
        outgoing.send(FromUi::Close).unwrap();
        let error = handles.ask("proceed?".to_owned()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn plan_holds_entries_warnings_and_first_run() {
        let plan = Plan {
            entries: vec![
                PlanEntry {
                    route: "config('mise/config.toml')".to_owned(),
                    kind: "toml".to_owned(),
                    action: PlanAction::Add,
                    changes: vec![Change::LeafValue {
                        key: "tools.node".to_owned(),
                        old: String::new(),
                        new: "22".to_owned(),
                        source: ChangeSource::Desired,
                    }],
                    drift: false,
                    rescue: false,
                },
                PlanEntry {
                    route: "home('.old')".to_owned(),
                    kind: "text".to_owned(),
                    action: PlanAction::Remove,
                    changes: Vec::new(),
                    drift: false,
                    rescue: false,
                },
            ],
            warnings: Vec::new(),
            first_run: true,
        };
        assert_eq!(plan.entries.len(), 2);
        assert_eq!(plan.entries[0].action, PlanAction::Add);
        assert!(plan.first_run);
        assert!(plan.warnings.is_empty());
    }
}
