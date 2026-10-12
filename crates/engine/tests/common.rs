//! Share builders for engine integration tests.

#![allow(dead_code)]
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use confit_engine::Engine;
use confit_engine::api::config::Config;
use confit_engine::api::patch::{Level, Patch, RcPatch, StructuredPatch};
use confit_engine::api::profile::Profile;
use confit_model::document::{Data, RcData, RcEntry, StructuredFormat};
use confit_model::hook::Hook;
use confit_model::routes::{Route, RouteBase};
use confit_store::blob::BlobSource;
use confit_store::{StoreRoots, Stores};
use mlua::{Function, Lua};

/// Lua profile with two text configs in zebra then apple order.
pub const PAIR: &str = r#"
        return {
            configs = {
                {
                    name = "first",
                    documents = { [{ base = "home", relative = "zebra" }] = { text = "z" } },
                },
                {
                    name = "second",
                    documents = { [{ base = "home", relative = "apple" }] = { text = "a" } },
                },
            },
        }
    "#;

/// Lua profile with repeated destination documents.
pub const DUPLICATE: &str = r#"
        return {
            configs = {
                { name = "first", documents = { [{ base = "home", relative = ".bashrc" }] = { text = "one" } } },
                { name = "second", documents = { [{ base = "home", relative = ".bashrc" }] = { text = "two" } } },
            },
        }
    "#;

/// Lua profile with one structured toml document.
pub const STRUCTURED: &str = r#"
        return {
            configs = {
                {
                    name = "tools/bat",
                    documents = {
                        [{ base = "home", relative = "config.toml" }] = {
                            format = "toml",
                            data = { profile = { shell = "bash" } },
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with repeated config names.
pub const DUPLICATE_NAMES: &str = r#"
        return {
            configs = {
                { name = "same", documents = { [{ base = "home", relative = "a" }] = { text = "one" } } },
                { name = "same", documents = { [{ base = "home", relative = "b" }] = { text = "two" } } },
            },
        }
    "#;

/// Lua profile with two structured patches over one base.
pub const MERGED_PATCHES: &str = r#"
        return {
            configs = {
                {
                    name = "tools/app",
                    documents = {
                        [{ base = "home", relative = "config.json" }] = {
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "json",
                            apply = function(_, d) d.data.alpha = "one" end,
                        },
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "json",
                            apply = function(_, d) d.data.beta = "two" end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one structured patch over a missing target.
pub const MISSING_TARGET: &str = r#"
        return {
            configs = {
                {
                    name = "tools/app",
                    patches = {
                        {
                            target = { base = "home", relative = "fresh.json" },
                            format = "json",
                            apply = function(_, d) d.data.made = "yes" end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one structured patch over a text base.
pub const TEXT_BASE: &str = r#"
        return {
            configs = {
                {
                    name = "tools/app",
                    documents = { [{ base = "home", relative = "note.txt" }] = { text = "hi" } },
                    patches = {
                        {
                            target = { base = "home", relative = "note.txt" },
                            format = "json",
                            apply = function(_, d) d.data.x = 1 end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one failing structured callback.
pub const FAILING_CALLBACK: &str = r#"
        return {
            configs = {
                {
                    name = "tools/app",
                    documents = {
                        [{ base = "home", relative = "config.json" }] = {
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "json",
                            apply = function(_, d) error("patch boom") end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with two rc patches over one base.
pub const RC_MERGED: &str = r#"
        return {
            configs = {
                {
                    name = "tools/shell",
                    documents = {
                        [{ base = "home", relative = ".bashrc" }] = {
                            profile = { { env = { name = "SEED", value = "0" } } },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = ".bashrc" },
                            apply = function(_, d) d.profile[#d.profile + 1] = { env = { name = "FIRST", value = "1" } } end,
                        },
                        {
                            target = { base = "home", relative = ".bashrc" },
                            apply = function(_, d) d.config[#d.config + 1] = { alias = { name = "ll", expansion = "ls -l" } } end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one rc patch over a missing target.
pub const RC_MISSING: &str = r#"
        return {
            configs = {
                {
                    name = "tools/shell",
                    patches = {
                        {
                            target = { base = "home", relative = ".bashrc" },
                            apply = function(_, d) d.profile[#d.profile + 1] = { env = { name = "MADE", value = "yes" } } end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one rc patch over a text base.
pub const RC_TEXT_BASE: &str = r#"
        return {
            configs = {
                {
                    name = "tools/shell",
                    documents = { [{ base = "home", relative = "note.txt" }] = { text = "hi" } },
                    patches = {
                        {
                            target = { base = "home", relative = "note.txt" },
                            apply = function(_, d) d.profile[#d.profile + 1] = { env = { name = "MADE", value = "yes" } } end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one failing rc callback.
pub const RC_FAILING: &str = r#"
        return {
            configs = {
                {
                    name = "tools/shell",
                    documents = {
                        [{ base = "home", relative = ".bashrc" }] = {
                            profile = { { env = { name = "SEED", value = "0" } } },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = ".bashrc" },
                            apply = function(_, d) error("rc boom") end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one structured patch over an rc base.
pub const RC_FORMAT_GAP: &str = r#"
        return {
            configs = {
                {
                    name = "tools/shell",
                    documents = {
                        [{ base = "home", relative = ".bashrc" }] = {
                            profile = { { env = { name = "SEED", value = "0" } } },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = ".bashrc" },
                            format = "json",
                            apply = function(_, d) d.data.x = 1 end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with a toml patch over a json base.
pub const FORMAT_GAP: &str = r#"
        return {
            configs = {
                {
                    name = "tools/app",
                    documents = {
                        [{ base = "home", relative = "config.json" }] = {
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "toml",
                            apply = function(_, d) d.data.x = 1 end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile reading project text through the resource bridge.
pub const PROJECT_TEXT: &str = r#"
        local resource = require("confit.resource")
        local handle = assert(resource("note.txt"))
        return {
            configs = {
                {
                    name = "tools/bat",
                    documents = { [{ base = "home", relative = ".bashrc" }] = { text = assert(handle:text()) } },
                },
            },
        }
    "#;

/// Lua profile with one patch adding count and extra keys.
pub const PATCHED: &str = r#"
        return {
            configs = {
                {
                    name = "tools/app",
                    documents = {
                        [{ base = "home", relative = "config.json" }] = {
                            format = "json",
                            data = { count = 1 },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "json",
                            apply = function(_, d) d.data.count = 2 d.data.extra = "yes" end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile mixing structured and rc patches over live targets.
pub const MIXED_SPLIT: &str = r#"
        return {
            configs = {
                {
                    name = "tools/both",
                    documents = {
                        [{ base = "home", relative = "config.json" }] = {
                            format = "json",
                            data = { seed = 0 },
                        },
                        [{ base = "home", relative = ".bashrc" }] = {
                            profile = { { env = { name = "SEED", value = "0" } } },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "json",
                            apply = function(_, d) d.data.alpha = "one" end,
                        },
                        {
                            target = { base = "home", relative = ".bashrc" },
                            apply = function(_, d) d.profile[#d.profile + 1] = { env = { name = "FIRST", value = "1" } } end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one rc patch over a structured base.
pub const RC_OVER_STRUCTURED: &str = r#"
        return {
            configs = {
                {
                    name = "tools/app",
                    documents = {
                        [{ base = "home", relative = "config.json" }] = {
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            apply = function(_, d) d.profile[#d.profile + 1] = { env = { name = "MADE", value = "yes" } } end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with mixed structured formats over one live target.
pub const MIXED_FORMATS: &str = r#"
        return {
            configs = {
                {
                    name = "tools/app",
                    documents = {
                        [{ base = "home", relative = "fresh.json" }] = {
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "fresh.json" },
                            format = "json",
                            apply = function(_, d) d.data.a = 1 end,
                        },
                        {
                            target = { base = "home", relative = "fresh.json" },
                            format = "toml",
                            apply = function(_, d) d.data.b = 2 end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with structured and rc patches over one structured base.
pub const MIXED_SAME_TARGET: &str = r#"
        return {
            configs = {
                {
                    name = "tools/app",
                    documents = {
                        [{ base = "home", relative = "config.json" }] = {
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "json",
                            apply = function(_, d) d.data.alpha = "one" end,
                        },
                        {
                            target = { base = "home", relative = "config.json" },
                            apply = function(_, d) d.profile[#d.profile + 1] = { env = { name = "MADE", value = "yes" } } end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with one patch carrying a foreign key.
pub const FOREIGN_PATCH_KEY: &str = r#"
        return {
            configs = {
                {
                    name = "tools/app",
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "json",
                            apply = function(_, d) return d end,
                            nope = 1,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile with merged hooks over one text document.
pub const HOOKS_THROUGH: &str = r#"
            return {
                configs = {
                    {
                        name = "tools/app",
                        documents = { [{ base = "home", relative = "note.txt" }] = { text = "hi" } },
                        hooks = {
                            { argv = { "mise", "install" } },
                            { argv = { "mise", "install" }, timeout = "90" },
                            { argv = { "fc-cache" }, timeout = "10m" },
                        },
                    },
                },
            }
        "#;

/// Build one text document pair under the home base.
pub fn placed_text(relative: &str, text: &str) -> (Route, Data) {
    (
        Route::new(RouteBase::Home, relative).unwrap(),
        Data::Text {
            content: text.to_owned(),
            mode: 0o644,
            unmanaged: false,
        },
    )
}

/// Build one rc document pair with profile entries.
pub fn placed_rc(relative: &str, entries: Vec<RcEntry>) -> (Route, Data) {
    (
        Route::new(RouteBase::Home, relative).unwrap(),
        Data::Rc(RcData::new(entries, Vec::new(), Vec::new())),
    )
}

/// Build one seeded json document pair.
pub fn placed_structured(relative: &str) -> (Route, Data) {
    (
        Route::new(RouteBase::Home, relative).unwrap(),
        Data::Structured {
            format: StructuredFormat::Json,
            data: serde_json::json!({"seed": 0}),
        },
    )
}

/// Build isolated stores over one fresh tempdir.
pub fn test_stores() -> (tempfile::TempDir, Stores) {
    let dir = tempfile::tempdir().unwrap();
    let (sender, _) = crossbeam_channel::unbounded();
    let stores = Stores::new(
        confit_store::StoreRoots {
            config_base: dir.path().join("config"),
            temp_base: dir.path().join("temp"),
            project_root: dir.path().join("project"),
        },
        sender,
    );
    (dir, stores)
}

/// Build one opaque document pair backed by a project blob.
pub fn placed_opaque(stores: &Stores, relative: &str) -> (Route, Data) {
    let root = stores.resources().roots.project_root.clone();
    confit_driver::fs::create_dir_all(&root).unwrap();
    let full = root.join("blob.bin");
    confit_driver::fs::write(&full, b"hi").unwrap();
    let resource = stores.resources().resource(&full).unwrap();
    let opened = stores.blobs().put(BlobSource::Handle(&resource)).unwrap();
    let size = stores.blobs().len(&opened).unwrap();
    (
        Route::new(RouteBase::Home, relative).unwrap(),
        Data::Opaque {
            blob: opened.to_ref(),
            size,
            mode: 0o644,
            unmanaged: true,
        },
    )
}

/// Build one profile.
pub fn profile_of(documents: Vec<(Route, Data)>, patches: Vec<Patch>) -> Profile {
    Profile::new(
        vec![Config::new(
            "tools/test".to_owned(),
            documents.into_iter().collect(),
            patches,
            Vec::new(),
        )],
        HashMap::new(),
    )
}

/// Build one env rc entry with its name and value.
pub fn rc_entry(name: &str, value: &str) -> RcEntry {
    RcEntry::Env {
        name: name.to_owned(),
        value: value.to_owned(),
        when: None,
    }
}

/// Build one structured patch over its Lua callback source.
pub fn structured_patch_of(
    lua: &Lua,
    relative: &str,
    format: StructuredFormat,
    priority: Level,
    source: &str,
) -> StructuredPatch {
    let apply: Function = lua.load(source).call(()).unwrap();
    StructuredPatch {
        target: Some(Route::new(RouteBase::Home, relative).unwrap()),
        filter: None,
        format,
        apply,
        priority,
    }
}

/// Build one rc patch over its Lua callback source.
pub fn rc_patch_of(lua: &Lua, relative: &str, priority: Level, source: &str) -> RcPatch {
    let apply: Function = lua.load(source).call(()).unwrap();
    RcPatch {
        target: Some(Route::new(RouteBase::Home, relative).unwrap()),
        filter: None,
        apply,
        priority,
    }
}

/// Build one structured filter patch over its Lua callback source.
pub fn structured_filter_of(
    lua: &Lua,
    filter_source: &str,
    format: StructuredFormat,
    priority: Level,
    source: &str,
) -> StructuredPatch {
    let filter: Function = lua.load(filter_source).call(()).unwrap();
    let apply: Function = lua.load(source).call(()).unwrap();
    StructuredPatch {
        target: None,
        filter: Some(filter),
        format,
        apply,
        priority,
    }
}

/// Build one rc filter patch over its Lua callback source.
pub fn rc_filter_of(lua: &Lua, filter_source: &str, priority: Level, source: &str) -> RcPatch {
    let filter: Function = lua.load(filter_source).call(()).unwrap();
    let apply: Function = lua.load(source).call(()).unwrap();
    RcPatch {
        target: None,
        filter: Some(filter),
        apply,
        priority,
    }
}

/// Collect profile slot names from one finished rc payload.
pub fn rc_names(data: &Data) -> Vec<String> {
    match data {
        Data::Rc(found) => found
            .profile
            .iter()
            .filter_map(|entry| entry.slot_name().map(str::to_owned))
            .collect(),
        _ => panic!("rc patch stays rc"),
    }
}

/// Collect one env value from the profile section by name.
pub fn rc_value(data: &Data, name: &str) -> Option<String> {
    match data {
        Data::Rc(found) => found.profile.iter().find_map(|entry| match entry {
            RcEntry::Env {
                name: key, value, ..
            } if key == name => Some(value.clone()),
            _ => None,
        }),
        _ => panic!("rc patch stays rc"),
    }
}

/// Build one hook with argv words and a timeout.
pub fn hook_of(argv: &[&str], timeout_secs: u64) -> Hook {
    Hook {
        argv: argv
            .iter()
            .map(|item| confit_model::arg::Arg::Text((*item).to_owned()))
            .collect(),
        path: Vec::new(),
        requires: None,
        when: None,
        checks: Vec::new(),
        timeout_secs,
    }
}

/// Build one profile.
pub fn profile_with_hooks(configs: Vec<Config>) -> Profile {
    Profile::new(configs, HashMap::new())
}

/// Build one engine over isolated stores under the given dir.
pub fn test_engine(dir: &Path) -> Engine {
    let (sender, _) = crossbeam_channel::unbounded();
    let stores = Stores::new(
        StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            project_root: dir.join("project"),
        },
        sender,
    );
    Engine::new(stores)
}

/// Create the project root under the given dir.
pub fn project_root(dir: &Path) -> PathBuf {
    let root = dir.join("project");
    confit_driver::fs::create_dir_all(&root).unwrap();
    root
}

/// Write one Lua profile file under the project root.
pub fn profile_with(dir: &Path, source: &str) -> (PathBuf, PathBuf) {
    let root = project_root(dir);
    let profile = root.join("profile.lua");
    confit_driver::fs::write(&profile, source.as_bytes()).unwrap();
    (root, profile)
}

/// Collect text content from one finished payload.
pub fn text_of(data: &Data) -> &str {
    match data {
        Data::Text { content, .. } => content,
        _ => panic!("expected a text document"),
    }
}
