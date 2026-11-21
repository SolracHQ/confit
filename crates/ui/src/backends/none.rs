//! Silent backend.

use confit_events::UiHandles;
use confit_events::protocol::{FromUi, ToUi};

/// Answers prompts.
pub(crate) fn start(handles: UiHandles) {
    while let Some(event) = handles.recv() {
        match event {
            ToUi::Prompt(_) => handles.send(FromUi::Answer(read_line())),
            ToUi::ShowAndConfirm { force, .. } => {
                if force {
                    handles.send(FromUi::Approved);
                } else {
                    handles.send(FromUi::Close);
                }
            }
            _ => {}
        }
    }
}

/// Reads one stdin line with the trailing newline trimmed.
fn read_line() -> String {
    use std::io::BufRead as _;
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(_) => line.trim_end_matches(['\r', '\n']).to_owned(),
        Err(_) => String::new(),
    }
}
