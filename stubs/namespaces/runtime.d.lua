---@meta _
-- Editor stubs for confit.runtime. The loader builds condition tables at runtime.
-- Shared shapes live in stubs/confit.d.lua.

---@class EnvEqOpts
---@field key string # Variable name.
---@field value string # Expected value.
-- Input for confit.runtime.env_eq.
local EnvEqOpts = {}

---@class EnvSetOpts
---@field key string # Variable name.
-- Input for confit.runtime.env_set.
local EnvSetOpts = {}

---@class RuntimeNs
---@field SHELL string # Template slot for the declared shell name, "{{shell}}". Materialized per shell at fold time.
-- Condition constructor namespace. Values guard rc entries through when opts.
local RuntimeNs = {}

-- Builds an env_eq condition table.
---@param opts EnvEqOpts # Variable name plus expected value.
---@return table
function RuntimeNs.env_eq(opts) end

-- Builds an env_set condition table.
---@param opts EnvSetOpts # Variable name.
---@return table
function RuntimeNs.env_set(opts) end

-- Builds an in_path condition table from a binary name.
---@param name string # Binary name, e.g. "bat".
---@return table
function RuntimeNs.in_path(name) end

-- Builds an exists condition table from a path.
---@param path table # Destination route, e.g. confit.path.home("tool").
---@return table
function RuntimeNs.exists(path) end

-- Builds a changed condition table from a document path.
---@param path table # Built document route watched for changes. Unknown documents fail the plan.
---@return table
function RuntimeNs.changed(path) end

-- Builds an all condition table.
---@param conds table[] # Condition tables, all holding.
---@return table
function RuntimeNs.all(conds) end

-- Builds an any condition table.
---@param conds table[] # Condition tables, one holding.
---@return table
function RuntimeNs.any(conds) end

-- Builds a nop condition table negating one condition.
---@param cond table # Condition table to negate.
---@return table
function RuntimeNs.nop(cond) end
