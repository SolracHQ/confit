---@meta _
-- Editor stubs for confit.hook. The loader builds hook tables at runtime.
-- Shared shapes live in stubs/confit.d.lua.

---@class HookOpts
---@field path? string[] # PATH extension dirs for the hook subprocess alone.
---@field when? table|fun(runtime: RuntimeNs): table # Run gate, or a builder function receiving confit.runtime.
---@field checks? table[] # Proof conditions. Passing checks skip the hook.
---@field timeout? string # Run cap as a Lua-shaped duration, e.g. "10m". Defaults to "10m".
-- Input for confit.hook.run.
local HookOpts = {}

---@class Hook
-- Post-config step table built by confit.hook.run. Attached with config:add_hook.
local Hook = {}

---@class HookNs
-- Post-config step namespace. Values ride configs beside documents plus patches.
local HookNs = {}

-- Builds a hook table running argv directly with no shell in between.
---@param argv string[] # Command plus arguments in order, e.g. { "mise", "install" }.
---@param opts HookOpts? # Path plus gate plus checks plus timeout shaping the run.
---@return Hook
function HookNs.run(argv, opts) end
