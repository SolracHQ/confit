---@meta _

---@class Config
-- Rust-owned config handle. Mutated in place by add_artifact.
local Config = {}

---@class RcOpts
---@field when? table|fun(shell: ShellNs): table # Guard condition table, or a builder function receiving confit.shell.
---@field priority? integer # Merge priority, higher wins. Defaults to 0.
-- Options for confit.artifact.rc constructors.
local RcOpts = {}

---@class InitSpec
---@field eval string[]? # Command argv evaluated as eval "$(argv...)". Exactly one of eval/cmd/source.
---@field cmd string[]? # Command argv run as a plain line. Exactly one of eval/cmd/source.
---@field source string? # File path sourced as source path. Exactly one of eval/cmd/source.
-- Init entry for confit.artifact.rc.init().
local InitSpec = {}

---@class Artifact
-- Artifact value built by confit.artifact constructors. Attached with config:add_artifact.
local Artifact = {}

---@class RcEntry
-- Rc entry handle built by confit.artifact.rc constructors. Attached with config:add_artifact.
local RcEntry = {}

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

---@class EnvEqOpts
---@field key string # Variable name.
---@field value string # Expected value.
-- Input for confit.shell.env_eq.
local EnvEqOpts = {}

---@class EnvSetOpts
---@field key string # Variable name.
-- Input for confit.shell.env_set.
local EnvSetOpts = {}

---@class Resources
-- File reads and table merge namespace.
local Resources = {}

---@class ArtifactNs
---@field rc RcNs # Rc entry table constructors namespace.
-- Artifact value constructors namespace.
local ArtifactNs = {}

---@class RcNs
-- Rc entry handle constructors namespace. Each entry goes to config:add_artifact.
local RcNs = {}

---@class ShellNs
---@field SHELL string # Template slot for the declared shell name, "{{shell}}". Materialized per shell at fold time.
-- Condition constructor namespace. Values guard rc entries through when opts.
local ShellNs = {}

---@class PathLib
-- Pure path helpers namespace.
local PathLib = {}

---@class PluginHelpers
-- Plan-domain error helper namespace.
local PluginHelpers = {}

---@class PluginNs
---@field helpers PluginHelpers
-- Lazy plugin namespace. User tables resolve on first access.
local PluginNs = {}

---@class Confit
---@field config fun(name: string): Config
---@field artifact ArtifactNs
---@field shell ShellNs
---@field resources Resources
---@field path PathLib
---@field plugin PluginNs
-- The global scripting object. Provides config handles plus value namespaces.
local Confit = {}

---@class Profile
---@field shells string[] # Shells to render, e.g. {"bash"}. One rc artifact per entry.
---@field configs Config[] # Config handles composed into this profile.
-- Profile return table. Extra keys are ignored.
local Profile = {}

-- Creates a config handle collecting contributions for the plan.
---@param name string # Config name, e.g. "bat". Must be unique per evaluation.
---@return Config
function Confit.config(name) end

-- Attaches a confit.artifact value or rc entry handle to the config.
---@param artifact Artifact|RcEntry # Value from a confit.artifact constructor, tuned by with_priority (and when for rc entries).
function Config:add_artifact(artifact) end

-- Sets the merge priority on a file artifact value. Chainable.
---@param n integer # Merge priority, higher wins.
---@return Artifact
function Artifact:with_priority(n) end

-- Sets the guard condition on an rc entry handle. Chainable.
---@param cond table|fun(shell: ShellNs): table # Guard condition table, or a builder function receiving confit.shell.
---@return RcEntry
function RcEntry:when(cond) end

-- Sets the merge priority on an rc entry handle. Chainable.
---@param n integer # Merge priority, higher wins.
---@return RcEntry
function RcEntry:with_priority(n) end

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
---@param overlay table # Overlay table, wins on overlap.
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

-- Builds one alias rc entry handle.
---@param name string # Alias name, e.g. "cat".
---@param value string # Alias expansion, e.g. "bat".
---@param opts RcOpts? # Optional guard plus priority. Both tunable later through methods.
---@return RcEntry
function RcNs.alias(name, value, opts) end

-- Builds one env rc entry handle.
---@param name string # Variable name.
---@param value string # Variable value.
---@param opts RcOpts? # Optional guard plus priority. Both tunable later through methods.
---@return RcEntry
function RcNs.env(name, value, opts) end

-- Builds one profile rc entry handle.
---@param name string # Variable name.
---@param value string # Variable value.
---@param opts RcOpts? # Optional guard plus priority. Both tunable later through methods.
---@return RcEntry
function RcNs.profile(name, value, opts) end

-- Builds one profile_path rc entry handle.
---@param dir string # Directory, e.g. "~/.cargo/bin".
---@param opts RcOpts? # Optional guard plus priority. Both tunable later through methods.
---@return RcEntry
function RcNs.profile_path(dir, opts) end

-- Builds one init rc entry handle.
---@param spec InitSpec # Table with exactly one of eval/cmd holding a string argv array, or source holding a string path.
---@param opts RcOpts? # Optional guard plus priority. Both tunable later through methods.
---@return RcEntry
function RcNs.init(spec, opts) end

-- Builds an env_eq condition table.
---@param opts EnvEqOpts # Variable name plus expected value.
---@return table
function ShellNs.env_eq(opts) end

-- Builds an env_set condition table.
---@param opts EnvSetOpts # Variable name.
---@return table
function ShellNs.env_set(opts) end

-- Builds an in_path condition table from a binary name.
---@param name string # Binary name, e.g. "bat".
---@return table
function ShellNs.in_path(name) end

-- Builds an exists condition table from a path.
---@param path string # File path.
---@return table
function ShellNs.exists(path) end

-- Builds an all condition table.
---@param conds table[] # Condition tables, all holding.
---@return table
function ShellNs.all(conds) end

-- Builds an any condition table.
---@param conds table[] # Condition tables, one holding.
---@return table
function ShellNs.any(conds) end

-- Builds a nop condition table negating one condition.
---@param cond table # Condition table to negate.
---@return table
function ShellNs.nop(cond) end

-- Raises a plan-domain error attributing the calling plugin file.
---@param message string # Error text.
function PluginHelpers.error(message) end

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
