//! Share builders for engine integration tests.

#![allow(dead_code)]
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use confit_binding::callback::Callback;
use confit_binding::value::Decode;
use confit_engine::Engine;
use confit_engine::api::config::Config;
use confit_engine::api::patch::{HookPatch, Level, Patch, RcPatch, StructuredPatch};
use confit_engine::api::profile::Profile;
use confit_model::arg::Arg;
use confit_model::condition::Condition;
use confit_model::document::{
    Data, Document, OpaqueDocument, Payload, RcData, RcEntry, StructuredDocument, StructuredFormat,
    TextDocument,
};
use confit_model::hook::Hook;
use confit_model::routes::{Route, RouteBase};
use confit_store::{StoreRoots, Stores};
use mlua::{Function, Lua, Value};

/// Lua profile with two text configs in zebra then apple order.
pub const PAIR: &str = r#"
        return {
            configs = {
                {
                    name = "first",
                    documents = { { path = { base = "home", relative = "zebra" }, text = "z" } },
                },
                {
                    name = "second",
                    documents = { { path = { base = "home", relative = "apple" }, text = "a" } },
                },
            },
        }
    "#;

/// Lua profile with repeated destination documents.
pub const DUPLICATE: &str = r#"
        return {
            configs = {
                { name = "first", documents = { { path = { base = "home", relative = ".bashrc" }, text = "one" } } },
                { name = "second", documents = { { path = { base = "home", relative = ".bashrc" }, text = "two" } } },
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
                        {
                            path = { base = "home", relative = "config.toml" },
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
                { name = "same", documents = { { path = { base = "home", relative = "a" }, text = "one" } } },
                { name = "same", documents = { { path = { base = "home", relative = "b" }, text = "two" } } },
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
                        {
                            path = { base = "home", relative = "config.json" },
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "json",
                            apply = function(d) d.data.alpha = "one" end,
                        },
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "json",
                            apply = function(d) d.data.beta = "two" end,
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
                            apply = function(d) d.data.made = "yes" end,
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
                    documents = { { path = { base = "home", relative = "note.txt" }, text = "hi" } },
                    patches = {
                        {
                            target = { base = "home", relative = "note.txt" },
                            format = "json",
                            apply = function(d) d.data.x = 1 end,
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
                        {
                            path = { base = "home", relative = "config.json" },
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "json",
                            apply = function(d) error("patch boom") end,
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
                        {
                            path = { base = "home", relative = ".bashrc" },
                            profile = { { env = { name = "SEED", value = "0" } } },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = ".bashrc" },
                            apply = function(d) d.profile[#d.profile + 1] = { env = { name = "FIRST", value = "1" } } end,
                        },
                        {
                            target = { base = "home", relative = ".bashrc" },
                            apply = function(d) d.config[#d.config + 1] = { alias = { name = "ll", expansion = "ls -l" } } end,
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
                            apply = function(d) d.profile[#d.profile + 1] = { env = { name = "MADE", value = "yes" } } end,
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
                    documents = { { path = { base = "home", relative = "note.txt" }, text = "hi" } },
                    patches = {
                        {
                            target = { base = "home", relative = "note.txt" },
                            apply = function(d) d.profile[#d.profile + 1] = { env = { name = "MADE", value = "yes" } } end,
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
                        {
                            path = { base = "home", relative = ".bashrc" },
                            profile = { { env = { name = "SEED", value = "0" } } },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = ".bashrc" },
                            apply = function(d) error("rc boom") end,
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
                        {
                            path = { base = "home", relative = ".bashrc" },
                            profile = { { env = { name = "SEED", value = "0" } } },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = ".bashrc" },
                            format = "json",
                            apply = function(d) d.data.x = 1 end,
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
                        {
                            path = { base = "home", relative = "config.json" },
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "toml",
                            apply = function(d) d.data.x = 1 end,
                        },
                    },
                },
            },
        }
    "#;

/// Lua profile reading project text through the resource bridge.
pub const PROJECT_TEXT: &str = r#"
        local open = require("confit.open")
        local handle = assert(open("note.txt"))
        return {
            configs = {
                {
                    name = "tools/bat",
                    documents = { { path = { base = "home", relative = ".bashrc" }, text = assert(handle:text()) } },
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
                        {
                            path = { base = "home", relative = "config.json" },
                            format = "json",
                            data = { count = 1 },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "json",
                            apply = function(d) d.data.count = 2 d.data.extra = "yes" end,
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
                        {
                            path = { base = "home", relative = "config.json" },
                            format = "json",
                            data = { seed = 0 },
                        },
                        {
                            path = { base = "home", relative = ".bashrc" },
                            profile = { { env = { name = "SEED", value = "0" } } },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "json",
                            apply = function(d) d.data.alpha = "one" end,
                        },
                        {
                            target = { base = "home", relative = ".bashrc" },
                            apply = function(d) d.profile[#d.profile + 1] = { env = { name = "FIRST", value = "1" } } end,
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
                        {
                            path = { base = "home", relative = "config.json" },
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            apply = function(d) d.profile[#d.profile + 1] = { env = { name = "MADE", value = "yes" } } end,
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
                        {
                            path = { base = "home", relative = "fresh.json" },
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "fresh.json" },
                            format = "json",
                            apply = function(d) d.data.a = 1 end,
                        },
                        {
                            target = { base = "home", relative = "fresh.json" },
                            format = "toml",
                            apply = function(d) d.data.b = 2 end,
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
                        {
                            path = { base = "home", relative = "config.json" },
                            format = "json",
                            data = { seed = 0 },
                        },
                    },
                    patches = {
                        {
                            target = { base = "home", relative = "config.json" },
                            format = "json",
                            apply = function(d) d.data.alpha = "one" end,
                        },
                        {
                            target = { base = "home", relative = "config.json" },
                            apply = function(d) d.profile[#d.profile + 1] = { env = { name = "MADE", value = "yes" } } end,
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
                            apply = function(d) return d end,
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
                        documents = { { path = { base = "home", relative = "note.txt" }, text = "hi" } },
                        hooks = {
                            { argv = { "mise", "install" } },
                            { argv = { "mise", "install" }, timeout = "90" },
                            { argv = { "fc-cache" }, timeout = "10m" },
                        },
                    },
                },
            }
        "#;

/// Build one text document with its destination path.
pub fn placed_text(relative: &str, text: &str) -> Document {
    Document::new(
        Route::new(RouteBase::Home, relative).unwrap(),
        Data::Text(TextDocument {
            content: text.to_owned(),
            mode: 0o644,
            unmanaged: false,
        }),
    )
}

/// Build one rc document with profile entries and its destination path.
pub fn placed_rc(relative: &str, entries: Vec<RcEntry>) -> Document {
    Document::new(
        Route::new(RouteBase::Home, relative).unwrap(),
        Data::Rc(RcData::new(entries, Vec::new(), Vec::new())),
    )
}

/// Build one seeded json document with its destination path.
pub fn placed_structured(relative: &str) -> Document {
    Document::new(
        Route::new(RouteBase::Home, relative).unwrap(),
        Data::Structured(StructuredDocument {
            format: StructuredFormat::Json,
            data: BTreeMap::from([("seed".to_owned(), serde_json::json!(0))]),
        }),
    )
}

/// Build isolated stores over one fresh tempdir.
pub fn test_stores() -> (tempfile::TempDir, Stores) {
    let dir = tempfile::tempdir().unwrap();
    let (sender, _) = crossbeam_channel::unbounded();
    let stores = Stores::new(
        StoreRoots {
            config_base: dir.path().join("config"),
            temp_base: dir.path().join("temp"),
            project_root: dir.path().join("project"),
        },
        sender,
    );
    (dir, stores)
}

/// Build one opaque document backed by a project blob with its destination path.
pub fn placed_opaque(stores: &Stores, relative: &str) -> Document {
    let root = stores.resources().roots.project_root.clone();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("blob.bin"), b"hi").unwrap();
    let resource = stores.resources().resource("blob.bin").unwrap();
    let opened = stores.blobs().put(&resource).unwrap();
    Document::new(
        Route::new(RouteBase::Home, relative).unwrap(),
        Data::Opaque(OpaqueDocument {
            handle: Payload::Resolved(opened.sha256().clone()),
            mode: 0o644,
            unmanaged: true,
        }),
    )
}

/// Build one profile with declared documents and patches.
pub fn profile_of(documents: Vec<Document>, patches: Vec<Patch>) -> Profile {
    Profile::new(
        vec![Config::new(
            "tools/test".to_owned(),
            documents,
            patches,
            Vec::new(),
            HashMap::new(),
        )],
        Vec::new(),
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
        apply: Callback::new(apply),
        priority,
    }
}

/// Build one rc patch over its Lua callback source.
pub fn rc_patch_of(lua: &Lua, relative: &str, priority: Level, source: &str) -> RcPatch {
    let apply: Function = lua.load(source).call(()).unwrap();
    RcPatch {
        target: Some(Route::new(RouteBase::Home, relative).unwrap()),
        filter: None,
        apply: Callback::new(apply),
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
        filter: Some(Callback::new(filter)),
        format,
        apply: Callback::new(apply),
        priority,
    }
}

/// Build one rc filter patch over its Lua callback source.
pub fn rc_filter_of(lua: &Lua, filter_source: &str, priority: Level, source: &str) -> RcPatch {
    let filter: Function = lua.load(filter_source).call(()).unwrap();
    let apply: Function = lua.load(source).call(()).unwrap();
    RcPatch {
        target: None,
        filter: Some(Callback::new(filter)),
        apply: Callback::new(apply),
        priority,
    }
}

/// Collect profile collision keys from one finished rc payload.
pub fn rc_names(data: &Data) -> Vec<String> {
    match data {
        Data::Rc(found) => found
            .profile
            .iter()
            .filter_map(|entry| entry.collision_key().map(str::to_owned))
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
            .map(|item| Arg::Text((*item).to_owned()))
            .collect(),
        path: Vec::new(),
        id: None,
        requires: Condition::default(),
        when: Condition::default(),
        check: Condition::default(),
        timeout_secs,
    }
}

/// Build one identified hook with argv words and a timeout.
pub fn identified_hook_of(id: &str, argv: &[&str], timeout_secs: u64) -> Hook {
    let mut hook = hook_of(argv, timeout_secs);
    hook.id = Some(id.to_owned());
    hook
}

/// Build one hook patch over its Lua callback source.
pub fn hook_patch_of(lua: &Lua, target: &str, source: &str) -> HookPatch {
    let apply: Function = lua.load(source).call(()).unwrap();
    let table = lua.create_table().unwrap();
    table.set("target", target).unwrap();
    table.set("apply", apply).unwrap();
    match HookPatch::decode(&Value::Table(table), lua) {
        Ok(patch) => patch,
        Err(error) => panic!("hook patch decodes: {error}"),
    }
}

/// Build one profile with hooks and zero documents.
pub fn profile_with_hooks(configs: Vec<Config>) -> Profile {
    Profile::new(configs, Vec::new())
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
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// Write one Lua profile file under the project root.
pub fn profile_with(dir: &Path, source: &str) -> (PathBuf, PathBuf) {
    let root = project_root(dir);
    let profile = root.join("profile.lua");
    std::fs::write(&profile, source.as_bytes()).unwrap();
    (root, profile)
}

/// Collect text content from one finished payload.
pub fn text_of(data: &Data) -> &str {
    match data {
        Data::Text(found) => &found.content,
        _ => panic!("expected a text document"),
    }
}
