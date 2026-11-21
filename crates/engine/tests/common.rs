//! Shares builders for the engine tests.

#![allow(clippy::unwrap_used)]

use confit_model::document::Data;

/// Returns the profile collision keys.
pub fn rc_names(data: &Data) -> Vec<String> {
    match data {
        Data::Rc(found) => found
            .profile
            .iter()
            .filter_map(|entry| entry.collision_key().map(str::to_owned))
            .collect(),
        _ => panic!("expected an rc document"),
    }
}
