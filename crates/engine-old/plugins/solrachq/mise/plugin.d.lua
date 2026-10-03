---@meta _
-- Editor stub for the solrachq.mise default plugin. This file ships
-- beside plugin.lua for language servers; the loader reads plugin.lua only.
-- Global scope mirrors what the Rust loader does: the plugin lands on
-- confit.plugin.solrachq.mise, so profiles see it transparently.

---@class MiseRcOpts
---@field when? table # Guard condition table. Merged beside the automatic in_path guard.
---@field section? string # Target rc section: "profile", "config", or "final". Defaults to "config".
-- Optional opts for collector methods. The section names the patch list,
-- the guard stays entry-level.
local MiseRcOpts = {}

---@class MiseRc
-- Scoped collector handed to the callback. Each method guards its entry
-- with `when = confit.runtime.in_path(binary)` plus adds it through a
-- batched into one patch.rc per callback run, in call order.
local MiseRc = {}

---@param name string # Alias name, e.g. "cat".
---@param value string # Alias expansion, e.g. "bat".
---@param opts MiseRcOpts? # Optional guard.
function MiseRc:alias(name, value, opts) end

---@param name string # Variable name.
---@param value string # Variable value.
---@param opts MiseRcOpts? # Optional guard.
function MiseRc:env(name, value, opts) end

---@param dir string # Directory, e.g. "~/.local/bin". Defaults the variable to PATH.
---@param opts MiseRcOpts? # Optional guard.
---@overload fun(self: MiseRc, var: string, dir: string, opts: MiseRcOpts?)
function MiseRc:prepend(dir, opts) end

---@param argv string[] # Command argv evaluated as eval "$(argv...)".
---@param opts MiseRcOpts? # Optional guard.
function MiseRc:eval(argv, opts) end

---@param argv string[] # Command argv run as a plain line.
---@param opts MiseRcOpts? # Optional guard.
function MiseRc:cmd(argv, opts) end

---@param path string # File path sourced as source path.
---@param opts MiseRcOpts? # Optional guard.
function MiseRc:source(path, opts) end

---@class MisePackageOpts
---@field name string # Package and config name, e.g. "bat".
---@field version? string # Pinned tool version folded into the shared TOML. Defaults to "latest".
---@field bin? string # Binary proving the install through its shim. Defaults to name.
---@field aliases? table<string, string> # Alias map rendered with the binary guard. Sorted by name.
---@field options? table<string, string|number|boolean|(string|number|boolean)[]> # Backend tool options folded into the shared TOML beside the version.
---@field rc_builder? fun(rc: MiseRc) # Optional builder receiving the collector.
-- Table shape for one package. Every field stays optional except `name`.
-- The package folds the version plus options, declares the install hook, requires
-- the installer config, then runs the builder.
local MisePackageOpts = {}

---@class MiseNs
-- Installer dialect namespace. `package` builds the config from a table,
-- `init` returns the installer config holding the binary, the base
-- document, plus the activation patch.
local MiseNs = {}

-- Declares one mise package plus its builder rc entries.
---@param opts MisePackageOpts # Package table holding name, version, options, rc_builder.
---@return table # Config userdata for the profile configs array.
function MiseNs.package(opts) end

-- Returns the installer config holding the shared base, the mise binary,
-- plus the activation patch.
---@param version? string # Pinned mise release, e.g. "2026.9.12". Omitted resolves the latest tag.
---@return table # Installer config userdata for the profile configs array.
function MiseNs.init(version) end

---@class SolrachqNs
-- Default plugin user table. Each field is one embedded plugin.
---@field mise MiseNs # Mise installer dialect namespace.
local SolrachqNs = {}

---@class PluginNs
---@field solrachq SolrachqNs # Default plugin user table.
-- Re-opened to attach the default user table; the loader fills it at runtime.

confit.plugin.solrachq.mise = MiseNs
