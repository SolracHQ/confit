---@meta _

---@class Config
-- Rust-owned config handle. Mutated in place by add_document and add_patch.
local Config = {}

---@class RcOpts
---@field when? table|fun(shell: ShellNs): table # Guard condition table, or a builder function receiving confit.shell.
---@field lane? string # Init lane, "first" or "last". Default middle. Stored on init entries only.
-- Options for confit.document.rc entry builders.
local RcOpts = {}

---@class RcSections
---@field profile? RcEntry[] # Setup entries: environment, profile, path, plus early init.
---@field config? RcEntry[] # Interactive entries: aliases.
---@field final? RcEntry[] # Final entries: init list in listed order.
-- Sections for confit.document.rc.new. Every key stays optional. Unknown keys are plan errors.
local RcSections = {}

---@class StructuredArgs
---@field path string # Destination path.
---@field data table # Data-only table.
-- Input for confit.document.structured. Unknown keys are plan errors.
local StructuredArgs = {}

---@class StructuredDocument
---@field path string # Destination path.
---@field data table # Data-only table.
-- Plain structured document table. Carries a structured marker.
local StructuredDocument = {}

---@class TextDocument
---@field path string # Destination path.
---@field content string # Exact file text.
-- Plain text document table. Carries a text marker.
local TextDocument = {}

---@class LinkDocument
---@field path string # Link path.
---@field target string # Link target.
-- Plain link document table. Carries a link marker.
local LinkDocument = {}

---@class RcDocument
---@field profile? RcEntry[] # Setup entries.
---@field config? RcEntry[] # Interactive entries.
---@field final? RcEntry[] # Final entries.
-- Plain rc document table. Carries an rc marker.
local RcDocument = {}

---@class Document : StructuredDocument, TextDocument, LinkDocument, RcDocument
-- Plain document table built by confit.document constructors. Attached with config:add_document.
local Document = {}

---@class AliasEntry
---@field name string # Alias name.
---@field value string # Alias expansion.
---@field when? table # Guard condition table.
-- Plain alias entry table. Carries an alias area marker.
local AliasEntry = {}

---@class EnvEntry
---@field name string # Variable name.
---@field value string # Variable value.
---@field when? table # Guard condition table.
-- Plain env entry table. Carries an env area marker.
local EnvEntry = {}

---@class ProfileEntry
---@field name string # Variable name.
---@field value string # Directory value.
---@field op string # Path operation, "prepend".
---@field when? table # Guard condition table.
-- Plain profile entry table. Carries a profile area marker.
local ProfileEntry = {}

---@class EvalEntry
---@field eval table # Eval spec with argv plus lane.
---@field when? table # Guard condition table.
-- Plain eval entry table. Carries an init area marker.
local EvalEntry = {}

---@class CmdEntry
---@field cmd table # Cmd spec with argv plus lane.
---@field when? table # Guard condition table.
-- Plain cmd entry table. Carries an init area marker.
local CmdEntry = {}

---@class SourceEntry
---@field source table # Source spec with path plus lane.
---@field when? table # Guard condition table.
-- Plain source entry table. Carries an init area marker.
local SourceEntry = {}

---@class RcEntry : AliasEntry, EnvEntry, ProfileEntry, EvalEntry, CmdEntry, SourceEntry
-- Plain rc entry table built by confit.document.rc constructors. Attached with config:add_document.
local RcEntry = {}

---@class Patch
-- Patch handle built by confit.patch constructors. Attached with config:add_patch.
local Patch = {}

---@class PatchProxy
-- Live document wrapper handed to patch callbacks. Wraps one live table.
local PatchProxy = {}

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
-- File reads namespace. Reads stay jailed to the project root.
local Resources = {}

---@class TextNs
-- Template rendering namespace.
local TextNs = {}

---@class DocumentNs
---@field rc RcNs # Rc entry table constructors namespace.
-- Document table constructors namespace.
local DocumentNs = {}

---@class RcNs
-- Rc entry table constructors namespace. Each entry goes to config:add_document.
local RcNs = {}

---@class PatchNs
-- Patch handle constructors namespace. Each patch goes to config:add_patch.
local PatchNs = {}

---@class Priority
---@field MINOR string # Lowest priority.
---@field LOW string # Low priority.
---@field NORMAL string # Default priority.
---@field HIGH string # High priority.
---@field MAJOR string # Highest priority.
-- Patch priority levels for confit.patch handles.
local Priority = {}

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
---@field document DocumentNs
---@field patch PatchNs
---@field priority Priority
---@field shell ShellNs
---@field resources Resources
---@field text TextNs
---@field path PathLib
---@field plugin PluginNs
-- The global scripting object. Provides config handles plus value namespaces.
local Confit = {}

---@class Profile
---@field shells string[] # Shells to render, e.g. {"bash"}. One rc document per entry.
---@field documents Document[]? # Machine-owned base documents, e.g. the rc document.
---@field configs Config[] # Config handles composed into this profile.
-- Profile return table. Extra keys are ignored.
local Profile = {}

-- Creates a config handle collecting contributions for the plan.
---@param name string # Config name, e.g. "bat". Must be unique per evaluation.
---@return Config
function Confit.config(name) end

-- Attaches a confit.document table or rc entry table to the config.
---@param document Document|RcEntry # Table from a confit.document constructor, tuned by when opts for rc entries.
function Config:add_document(document) end

-- Attaches a confit.patch value to the config, stamping the config name as owner.
---@param patch Patch # Value from a confit.patch constructor, tuned by priority.
function Config:add_patch(patch) end

-- Sets the merge priority on a patch handle. Chainable.
---@param level string # One confit.priority level. Defaults to NORMAL.
---@return Patch
function Patch:priority(level) end

-- Writes one path through the patch proxy.
---@param path string # Dotted keys for structured documents, one of profile/config/final for rc documents.
---@param value any # Data-only value, or an rc entry table for rc documents.
function PatchProxy:set(path, value) end

-- Extends one list through the patch proxy.
---@param path string # Dotted keys for structured documents, one of profile/config/final for rc documents.
---@param value any # Data-only value, or an rc entry table for rc documents.
function PatchProxy:append(path, value) end

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

-- Reads a root-relative text file into a Lua string.
---@param path string # Project-root-relative path, e.g. "resources/starship.toml.j2".
---@return string
function Resources.load_text(path) end

-- Renders a template string with a vars table.
---@param template string # Template text with minijinja slots.
---@param vars table # Variables feeding template slots.
---@return string
function TextNs.render(template, vars) end

-- Builds a structured document table from format plus args.
---@param format string # One of "json", "toml", or "yaml".
---@param args StructuredArgs # Destination path plus data-only table.
---@return Document
function DocumentNs.structured(format, args) end

-- Builds a literal text document table.
---@param path string # Destination path.
---@param content string # Exact file text.
---@return Document
function DocumentNs.text(path, content) end

-- Builds a symlink document table.
---@param path string # Link path.
---@param target string # Link target.
---@return Document
function DocumentNs.link(path, target) end

-- Builds the single rc document table from section lists.
---@param sections RcSections # Optional profile/config/final entry-table lists.
---@return Document
function RcNs.new(sections) end

-- Builds one alias rc entry table.
---@param name string # Alias name, e.g. "cat".
---@param value string # Alias expansion, e.g. "bat".
---@param opts RcOpts? # Optional guard plus lane.
---@return RcEntry
function RcNs.alias(name, value, opts) end

-- Builds one env rc entry table.
---@param name string # Variable name.
---@param value string # Variable value.
---@param opts RcOpts? # Optional guard plus lane.
---@return RcEntry
function RcNs.env(name, value, opts) end

-- Builds one profile rc entry table.
---@param name string # Variable name.
---@param value string # Variable value.
---@param opts RcOpts? # Optional guard plus lane.
---@return RcEntry
function RcNs.profile(name, value, opts) end

-- Builds one profile_path rc entry table.
---@param dir string # Directory, e.g. "~/.cargo/bin".
---@param opts RcOpts? # Optional guard plus lane.
---@return RcEntry
function RcNs.profile_path(dir, opts) end

-- Builds one path_entry rc entry table, prepend sugar for PATH.
---@param dir string # Directory, e.g. "~/.cargo/bin".
---@param opts RcOpts? # Optional guard plus lane.
---@return RcEntry
function RcNs.path_entry(dir, opts) end

-- Builds one eval init rc entry table.
---@param argv string[] # Command argv evaluated as eval "$(argv...)".
---@param opts RcOpts? # Optional guard plus lane.
---@return RcEntry
function RcNs.eval(argv, opts) end

-- Builds one cmd init rc entry table.
---@param argv string[] # Command argv run as a plain line.
---@param opts RcOpts? # Optional guard plus lane.
---@return RcEntry
function RcNs.cmd(argv, opts) end

-- Builds one source init rc entry table.
---@param path string # File path sourced as source path.
---@param opts RcOpts? # Optional guard plus lane.
---@return RcEntry
function RcNs.source(path, opts) end

-- Builds an rc patch handle carrying the callback for live execution.
---@param callback fun(document: PatchProxy) # Callback running set and append live.
---@return Patch
function PatchNs.rc(callback) end

-- Builds a structured patch handle carrying the callback for live execution.
---@param format string # One of "json", "toml", or "yaml".
---@param path string # Target document path.
---@param callback fun(data: PatchProxy) # Callback running set and append live.
---@return Patch
function PatchNs.structured(format, path, callback) end

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
