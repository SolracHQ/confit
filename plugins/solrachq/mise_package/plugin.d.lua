---@meta _
-- Editor stub for the solrachq.mise_package default plugin. This file ships
-- beside plugin.lua for language servers; the loader reads plugin.lua only.
-- Global scope mirrors what the Rust loader does: the plugin lands on
-- confit.plugin.solrachq.mise_package, so profiles see it transparently.

---@class MisePackageRcOpts
---@field priority integer? # Merge priority passthrough. The in_path guard always applies.
-- Optional opts for collector methods.
local RcOpts = {}

---@class MisePackageRc
-- Scoped collector handed to the callback. Each method guards its entry
-- with `when = confit.shell.in_path(binary)`.
local Rc = {}

---@param name string # Alias name, e.g. "cat".
---@param value string # Alias expansion, e.g. "bat".
---@param opts MisePackageRcOpts? # Optional priority passthrough.
function Rc:alias(name, value, opts) end

---@param name string # Variable name.
---@param value string # Variable value.
---@param opts MisePackageRcOpts? # Optional priority passthrough.
function Rc:env(name, value, opts) end

---@param name string # Variable name.
---@param value string # Variable value.
---@param opts MisePackageRcOpts? # Optional priority passthrough.
function Rc:profile(name, value, opts) end

---@param dir string # Directory, e.g. "~/.local/bin".
---@param opts MisePackageRcOpts? # Optional priority passthrough.
function Rc:profile_path(dir, opts) end

---@param spec table # Init spec with exactly one of eval/cmd/source.
---@param opts MisePackageRcOpts? # Optional priority passthrough.
function Rc:init(spec, opts) end

-- Declares one mise package plus its callback rc entries.
---@param name string # Package and config name, e.g. "bat".
---@param callback fun(rc: MisePackageRc)? # Optional callback receiving the collector.
---@return table # Config userdata for the profile configs array.
local function mise_package(name, callback) end

---@class SolrachqNs
-- Default plugin user table. Each field is one embedded plugin.
---@field mise_package fun(name: string, callback?: fun(rc: MisePackageRc)): table # Declares one mise package plus its callback rc entries.
local Solrachq = {}

---@class PluginNs
---@field solrachq SolrachqNs # Default plugin user table.
-- Re-opened to attach the default user table; the loader fills it at runtime.

confit.plugin.solrachq.mise_package = mise_package
