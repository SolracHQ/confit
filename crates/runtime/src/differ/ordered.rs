//! Ordered sides for difference entries.

use std::collections::BTreeMap;

use confit_model::routes::Route;

use super::Difference;
use super::DifferenceOrder;

/// Ordered leaf values.
pub(crate) struct Sides {
    /// The old leaf value.
    pub(crate) old: serde_json::Value,
    /// The new leaf value.
    pub(crate) new: serde_json::Value,
}

/// Difference side order.
pub(crate) struct Ordered {
    /// The side order under comparing.
    order: DifferenceOrder,
}

impl Ordered {
    /// Build one ordered swapper.
    pub(crate) fn new(order: DifferenceOrder) -> Self {
        Self { order }
    }

    /// Pair one recorded string with one disk string in order.
    fn pair(&self, recorded: String, disk: String) -> Sides {
        let (old, new) = match self.order {
            DifferenceOrder::RecordedFirst => (recorded, disk),
            DifferenceOrder::DiskFirst => (disk, recorded),
        };
        Sides {
            old: serde_json::Value::String(old),
            new: serde_json::Value::String(new),
        }
    }

    /// Build one key entry from recorded and disk labels.
    pub(crate) fn key(&self, path: &Route, key: &str, recorded: &str, disk: &str) -> Difference {
        let sides = self.pair(recorded.to_string(), disk.to_string());
        Difference::Key {
            path: path.clone(),
            key: key.to_string(),
            old: Some(sides.old),
            new: Some(sides.new),
        }
    }

    /// Build one mode entry from wanted and seen bits.
    pub(crate) fn mode_key(&self, path: &Route, key: String, wanted: u32, seen: u32) -> Difference {
        let sides = self.pair(format!("{wanted:o}"), format!("{seen:o}"));
        Difference::Key {
            path: path.clone(),
            key,
            old: Some(sides.old),
            new: Some(sides.new),
        }
    }

    /// Build one text diff entry from recorded and disk bytes.
    pub(crate) fn text_diff(&self, path: &Route, recorded: &[u8], disk: &[u8]) -> Difference {
        let (first, second) = match self.order {
            DifferenceOrder::RecordedFirst => (recorded, disk),
            DifferenceOrder::DiskFirst => (disk, recorded),
        };
        Difference::Diff {
            path: path.clone(),
            diff: Self::strip_headers(
                &String::from_utf8_lossy(first),
                &String::from_utf8_lossy(second),
            ),
        }
    }

    /// Report one path-level entry for differing structured maps.
    pub(crate) fn structured(
        &self,
        path: &Route,
        recorded: &BTreeMap<String, serde_json::Value>,
        disk: &BTreeMap<String, serde_json::Value>,
    ) -> Vec<Difference> {
        let (old, new) = match self.order {
            DifferenceOrder::RecordedFirst => (recorded, disk),
            DifferenceOrder::DiskFirst => (disk, recorded),
        };
        if old == new {
            Vec::new()
        } else {
            vec![Difference::Structured {
                path: path.clone(),
                old: old.clone(),
                new: new.clone(),
            }]
        }
    }

    /// Build render-ready diff content from first text to second text.
    fn strip_headers(first: &str, second: &str) -> String {
        diffy::create_patch(first, second)
            .to_string()
            .lines()
            .filter(|line| {
                !(line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@"))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn literal(path: &std::path::Path) -> Route {
        use confit_model::routes::RouteBase;

        Route::new(RouteBase::Literal, path).unwrap()
    }

    #[test]
    fn key_swaps_sides_with_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = literal(&dir.path().join("file.txt"));
        let recorded_first =
            Ordered::new(DifferenceOrder::RecordedFirst).key(&path, "content", "rec", "disk");
        let disk_first =
            Ordered::new(DifferenceOrder::DiskFirst).key(&path, "content", "rec", "disk");
        match (recorded_first, disk_first) {
            (
                Difference::Key {
                    old: Some(a),
                    new: Some(b),
                    ..
                },
                Difference::Key {
                    old: Some(c),
                    new: Some(d),
                    ..
                },
            ) => {
                assert_eq!(a, serde_json::Value::String("rec".to_string()));
                assert_eq!(b, serde_json::Value::String("disk".to_string()));
                assert_eq!(c, serde_json::Value::String("disk".to_string()));
                assert_eq!(d, serde_json::Value::String("rec".to_string()));
            }
            _ => panic!("ordered keys carry sides"),
        }
    }
}
