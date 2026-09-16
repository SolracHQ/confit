---@meta _
-- Editor stubs for confit.patch plus confit.priority. The loader builds patch handles at runtime.
-- Shared shapes live in stubs/confit.d.lua.

---@class RcPatch
-- Live rc document handle handed to confit.patch.rc callbacks. Exposes add only.
local RcPatch = {}

---@class StructuredPatch
-- Live structured document handle handed to confit.patch.structured callbacks. Exposes set plus append only.
local StructuredPatch = {}

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

-- Sets the merge priority on a patch handle. Chainable.
---@param level string # One confit.priority level. Defaults to NORMAL.
---@return Patch
function Patch:priority(level) end

-- Writes one path through the structured patch handle.
---@param path string # Dotted keys for structured documents.
---@param value any # Data-only value.
function StructuredPatch:set(path, value) end

-- Extends one list through the structured patch handle.
---@param path string # Dotted keys for structured documents.
---@param value any # Data-only value.
function StructuredPatch:append(path, value) end

-- Adds one rc entry through the rc patch handle.
---@param section string # One of profile/config/final for rc documents.
---@param entry RcEntry # An rc entry table.
function RcPatch:add(section, entry) end

-- Builds an rc patch handle carrying the callback for live execution.
---@param callback fun(document: RcPatch) # Callback running add live.
---@return Patch
function PatchNs.rc(callback) end

-- Builds a structured patch handle carrying the callback for live execution.
---@param format string # One of "json", "toml", or "yaml".
---@param path string # Target document path.
---@param callback fun(data: StructuredPatch) # Callback running set and append live.
---@return Patch
function PatchNs.structured(format, path, callback) end
