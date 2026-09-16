---@meta _
-- Editor stubs for confit.config. The loader builds config handles at runtime.
-- Shared shapes live in stubs/confit.d.lua.

---@class Config
-- Rust-owned config handle. Mutated in place by add_document and add_patch.
local Config = {}

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
