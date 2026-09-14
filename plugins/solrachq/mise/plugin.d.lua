---@meta _
-- Editor stub for the solrachq.mise default plugin. This file ships
-- beside plugin.lua for language servers; the loader reads plugin.lua only.
-- Global scope mirrors what the Rust loader does: the plugin lands on
-- confit.plugin.solrachq.mise, so profiles see it transparently.

---@class MiseRcOpts
---@field lane string? # Init lane passthrough ("first" or "last"). The in_path guard always applies.
-- Optional opts for collector methods.
local RcOpts = {}

---@class MiseRc
-- Scoped collector handed to the callback. Each method guards its entry
-- with `when = confit.shell.in_path(binary)`.
local Rc = {}

---@param name string # Alias name, e.g. "cat".
---@param value string # Alias expansion, e.g. "bat".
---@param opts MiseRcOpts? # Optional lane passthrough.
function Rc:alias(name, value, opts) end

---@param name string # Variable name.
---@param value string # Variable value.
---@param opts MiseRcOpts? # Optional lane passthrough.
function Rc:env(name, value, opts) end

---@param name string # Variable name.
---@param value string # Variable value.
---@param opts MiseRcOpts? # Optional lane passthrough.
function Rc:profile(name, value, opts) end

---@param dir string # Directory, e.g. "~/.local/bin".
---@param opts MiseRcOpts? # Optional lane passthrough.
function Rc:profile_path(dir, opts) end

---@param argv string[] # Command argv evaluated as eval "$(argv...)".
---@param opts MiseRcOpts? # Optional lane passthrough.
function Rc:eval(argv, opts) end

---@param argv string[] # Command argv run as a plain line.
---@param opts MiseRcOpts? # Optional lane passthrough.
function Rc:cmd(argv, opts) end

---@param path string # File path sourced as source path.
---@param opts MiseRcOpts? # Optional lane passthrough.
function Rc:source(path, opts) end

---@class MiseNs
-- Installer dialect namespace. `package` builds the config, `activate`
-- returns the eval entry for mise activation.
local Mise = {}

-- Declares one mise package plus its callback rc entries.
---@param name string # Package and config name, e.g. "bat".
---@param callback fun(rc: MiseRc)? # Optional callback receiving the collector.
---@return table # Config userdata for the profile configs array.
function Mise.package(name, callback) end

-- Returns the eval entry for mise activation with the `{{shell}}` slot.
---@param opts MiseRcOpts? # Optional lane passthrough.
---@return table # Rc entry table for config:add_document.
function Mise.activate(opts) end

---@class SolrachqNs
-- Default plugin user table. Each field is one embedded plugin.
---@field mise MiseNs # Mise installer dialect namespace.
local Solrachq = {}

---@class PluginNs
---@field solrachq SolrachqNs # Default plugin user table.
-- Re-opened to attach the default user table; the loader fills it at runtime.

confit.plugin.solrachq.mise = Mise
