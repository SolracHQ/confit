---@meta _
-- Editor stubs for confit.plugin. The loader fills user tables at runtime.
-- Shared shapes live in stubs/confit.d.lua.
-- Each plugin ships its own stubs beside its plugin.lua.

---@class PluginHelpers
-- Plan-domain error helper namespace.
local PluginHelpers = {}

---@class PluginNs
---@field helpers PluginHelpers
-- Lazy plugin namespace. User tables resolve on first access.
-- Each plugin ships its own stubs beside its plugin.lua.
local PluginNs = {}

-- Raises a plan-domain error attributing the calling plugin file.
---@param message string # Error text.
function PluginHelpers.error(message) end
