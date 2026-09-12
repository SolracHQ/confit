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
---@field eval string[]? # Command argv evaluated as eval "$(argv...)". Exactly one of eval/cmd/source.
---@field cmd string[]? # Command argv run as a plain line. Exactly one of eval/cmd/source.
---@field source string? # File path sourced as source path. Exactly one of eval/cmd/source.
-- Init entry for tool:init().
local InitSpec = {}

---@class Tool
-- Rust-owned tool handle. Mutated in place by the method calls.
local Tool = {}

---@class Artifact
-- Artifact value built by confit.artifact constructors. Attached with tool:append_artifact.
local Artifact = {}

---@class MergeOpts
---@field shallow boolean? # Merge top-level keys only. Defaults to false.
---@field list_append boolean? # Concatenate arrays instead of replacing. Defaults to false.
-- Options for confit.resources.merge. Unknown keys are plan errors.
local MergeOpts = {}

---@class TemplateOpts
---@field src string # Inline content or project-root-relative path.
---@field vars table? # Render variables.
-- Input for confit.artifact.template.
local TemplateOpts = {}

---@class Resources
-- File reads and table merge namespace.
local Resources = {}

---@class ArtifactNs
-- Artifact value constructors namespace.
local ArtifactNs = {}

---@class PathLib
-- Pure path helpers namespace.
local PathLib = {}

---@class Mise
-- Installer provider namespace.
local Mise = {}

---@class Confit
---@field mise Mise
---@field resources Resources
---@field artifact ArtifactNs
---@field path PathLib
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
---@param spec InitSpec # Table with exactly one of eval/cmd holding a string argv array, or source holding a string path.
function Tool:init(spec) end

-- Attaches a confit.artifact value to the tool. Merged by (kind, path).
---@param artifact Artifact # Value from a confit.artifact constructor.
function Tool:append_artifact(artifact) end

-- Reads a root-relative TOML file into a Lua table.
---@param path string # Project-root-relative path, e.g. "resources/starship.toml".
---@return table
function Resources.load_toml(path) end

-- Reads a root-relative JSON file into a Lua table.
---@param path string # Project-root-relative path.
---@return table
function Resources.load_json(path) end

-- Reads a root-relative YAML file into a Lua table.
---@param path string # Project-root-relative path.
---@return table
function Resources.load_yaml(path) end

-- Deep-merges overlay over base. Tables recurse, everything else last-wins.
---@param base table # Base table.
---@param overlay table # Overlay table, wins on conflict.
---@param opts MergeOpts? # Optional tweaks.
---@return table
function Resources.merge(base, overlay, opts) end

-- Builds a TOML artifact value from a data table.
---@param path string # Destination path.
---@param data table # Data-only table.
---@return Artifact
function ArtifactNs.toml(path, data) end

-- Builds a JSON artifact value from a data table.
---@param path string # Destination path.
---@param data table # Data-only table.
---@return Artifact
function ArtifactNs.json(path, data) end

-- Builds a YAML artifact value from a data table.
---@param path string # Destination path.
---@param data table # Data-only table.
---@return Artifact
function ArtifactNs.yaml(path, data) end

-- Builds a literal file artifact value.
---@param path string # Destination path.
---@param content string # Exact file text.
---@return Artifact
function ArtifactNs.file(path, content) end

-- Builds a template artifact value.
---@param path string # Destination path.
---@param opts TemplateOpts # Source plus variables.
---@return Artifact
function ArtifactNs.template(path, opts) end

-- Builds a symlink artifact value.
---@param path string # Link path.
---@param target string # Link target.
---@return Artifact
function ArtifactNs.link(path, target) end

-- Joins $HOME with the segments.
---@param ... string # Path segments.
---@return string
function PathLib.home(...) end

-- Joins $XDG_CONFIG_HOME with the segments. Where managed files land.
---@param ... string # Path segments.
---@return string
function PathLib.config(...) end

-- Joins $XDG_DATA_HOME with the segments.
---@param ... string # Path segments.
---@return string
function PathLib.data(...) end

-- Joins the confit project root with the segments. Where files come from.
-- Kept for symlinks to shipped resources.
---@param ... string # Path segments.
---@return string
function PathLib.confroot(...) end

confit = Confit
