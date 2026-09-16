---@meta _

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

---@class OpaqueDocument
---@field path string # Destination path.
---@field content string # Raw file bytes as a Lua string.
-- Plain opaque document table. Carries an opaque marker.
local OpaqueDocument = {}

---@class RcDocument
---@field profile? RcEntry[] # Entries rendering before the guard.
---@field config? RcEntry[] # Entries rendering after the guard.
---@field final? RcEntry[] # Entries rendering last.
-- Plain rc document table. Carries an rc marker.
local RcDocument = {}

---@class Document : StructuredDocument, TextDocument, LinkDocument, RcDocument, OpaqueDocument
-- Plain document table built by confit.document constructors. Attached with config:add_document.
local Document = {}

---@class AliasEntry
---@field alias table # Alias op with name plus expansion.
---@field when? table # Guard condition table.
-- Plain alias entry table. Carries an rc-entry marker.
local AliasEntry = {}

---@class EnvEntry
---@field env table # Env op with name plus value.
---@field when? table # Guard condition table.
-- Plain env entry table. Carries an rc-entry marker.
local EnvEntry = {}

---@class PathEntry
---@field path table # Path op with name plus dir plus op ("prepend").
---@field when? table # Guard condition table.
-- Plain path entry table. Carries an rc-entry marker.
local PathEntry = {}

---@class EvalEntry
---@field eval table # Eval op with argv.
---@field when? table # Guard condition table.
-- Plain eval entry table. Carries an rc-entry marker.
local EvalEntry = {}

---@class CmdEntry
---@field cmd table # Cmd op with argv.
---@field when? table # Guard condition table.
-- Plain cmd entry table. Carries an rc-entry marker.
local CmdEntry = {}

---@class SourceEntry
---@field source table # Source op with path.
---@field when? table # Guard condition table.
-- Plain source entry table. Carries an rc-entry marker.
local SourceEntry = {}

---@class RcEntry : AliasEntry, EnvEntry, PathEntry, EvalEntry, CmdEntry, SourceEntry
-- Plain rc entry table built by confit.document.rc constructors. Attached with config:add_document.
local RcEntry = {}

---@class Patch
-- Patch handle built by confit.patch constructors. Attached with config:add_patch.
local Patch = {}

---@class Confit
---@field config fun(name: string): Config
---@field document DocumentNs
---@field patch PatchNs
---@field priority Priority
---@field shell ShellNs
---@field resources Resources
---@field utils UtilsNs
---@field path PathLib
---@field plugin PluginNs
-- The global scripting object. Provides config handles plus value namespaces.
local Confit = {}

---@class Profile
---@field shells string[] # Shells to render, e.g. {"bash"}. One rc document per entry.
---@field documents Document[]? # Machine-owned base documents, e.g. the rc document.
---@field configs Config[] # Config handles composed into this profile.
-- Profile return table. A parametrizing function returning this table also reads valid.
local Profile = {}

confit = Confit
