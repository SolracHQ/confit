---@meta _
-- Editor stubs for confit.config. The loader builds config handles at runtime.
-- Shared shapes live in stubs/confit.d.lua.

---@class Config
-- Engine-owned config handle. Mutated in place by add_document, add_patch,
-- add_hook, and require.
local Config = {}

-- Creates a config handle collecting contributions for the plan.
---@param name string # Config name, e.g. "bat". Must be unique per evaluation.
---@return Config
function confit.config(name) end

-- Attaches a confit.document table or rc entry table to the config.
---@param document Document|RcEntry # Table from a confit.document constructor, tuned by when opts for rc entries.
function Config:add_document(document) end

-- Attaches a confit.patch value to the config, stamping the config name as owner.
---@param patch Patch # Value from a confit.patch constructor, tuned by priority.
function Config:add_patch(patch) end

-- Attaches a confit.hook value to the config for post-config steps.
---@param hook Hook # Table from confit.hook.run, holding argv plus opts.
function Config:add_hook(hook) end

-- Requires a sibling config by name. Missing targets fail the plan.
---@param name string # Required config name, e.g. "plugin:solrachq/mise:install".
---@param hint? string # Optional hint printed on its own line while the target misses.
function Config:require(name, hint) end
