---@meta _
-- Editor stubs for confit.shell. The loader builds condition tables at runtime.
-- Shared shapes live in stubs/confit.d.lua.

---@class EnvEqOpts
---@field key string # Variable name.
---@field value string # Expected value.
-- Input for confit.shell.env_eq.
local EnvEqOpts = {}

---@class EnvSetOpts
---@field key string # Variable name.
-- Input for confit.shell.env_set.
local EnvSetOpts = {}

---@class ShellNs
---@field SHELL string # Template slot for the declared shell name, "{{shell}}". Materialized per shell at fold time.
-- Condition constructor namespace. Values guard rc entries through when opts.
local ShellNs = {}

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
