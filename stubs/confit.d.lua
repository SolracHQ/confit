---@meta _

---@class MiseSpec
---@field name string # Package name, e.g. "bat".
---@field version string # Pinned version, or "latest" when omitted.
-- Package spec returned by confit.mise.package().
local MiseSpec = {}

---@class MisePackageOpts
---@field name string # Package name, e.g. "bat".
---@field version string? # Pinned version. Defaults to "latest".
-- Input for confit.mise.package().
local MisePackageOpts = {}

---@class ToolOpts
---@field install table? # The confit.mise.package table, or nil for no installer entry.
-- Options for confit.tool().
local ToolOpts = {}

---@class InitSpec
---@field eval string[]? # Command argv evaluated as eval "$(argv...)". Exactly one of eval/cmd.
---@field cmd string[]? # Command argv run as a plain line. Exactly one of eval/cmd.
-- Init entry for tool:init().
local InitSpec = {}

---@class Tool
-- Rust-owned tool handle. Mutated in place by the :alias, :env, :profile and :init calls.
local Tool = {}

---@class Mise
-- Installer provider namespace.
local Mise = {}

---@class Confit
---@field mise Mise
-- The global scripting object. Provides tool handles and installer providers.
local Confit = {}

---@class Profile
---@field shells string[] # Shells to render, e.g. {"bash"}. One rc artifact per entry.
---@field tools Tool[] # Tool handles composed into this profile.
-- Profile return table. Extra keys are ignored.
local Profile = {}

-- Creates a mise package spec contributing an entry to the mise.toml artifact.
---@param spec MisePackageOpts # Table with name and optional version.
---@return MiseSpec
function Mise.package(spec) end

-- Creates a tool handle collecting contributions for the plan.
---@param name string # Tool name, e.g. "bat".
---@param opts ToolOpts? # Optional table; opts.install must be the confit.mise.package table.
---@return Tool
function Confit.tool(name, opts) end

-- Contributes a shell alias to every declared shell rc.
---@param name string # Alias name, e.g. "cat".
---@param value string # Alias expansion, e.g. "bat".
function Tool:alias(name, value) end

-- Contributes an interactive shell variable to every declared shell rc.
---@param name string # Variable name.
---@param value string # Variable value.
function Tool:env(name, value) end

-- Contributes an always-loaded variable entry to the profile file.
---@param name string # Variable name.
---@param value string # Variable value.
function Tool:profile(name, value) end

-- Prepends a directory to PATH in the always-loaded profile file.
---@param dir string # Directory, e.g. "~/.cargo/bin".
function Tool:profile_path(dir) end

-- Contributes a shell init entry.
---@param spec InitSpec # Table with exactly one of eval or cmd holding a string argv array.
function Tool:init(spec) end

confit = Confit
