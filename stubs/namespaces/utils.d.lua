---@meta _
-- Editor stubs for confit.utils. The loader renders templates plus checks table shapes at runtime.
-- Shared shapes live in stubs/confit.d.lua.

---@class UtilsNs
-- Template rendering plus table shape namespace.
local UtilsNs = {}

-- Renders a template string with a vars table.
---@param template string # Template text with minijinja slots.
---@param vars table # Variables feeding template slots.
---@return string
function UtilsNs.render(template, vars) end

-- Reports true when a value holds a table cycle.
---@param value any # Value to inspect for ancestor cycles.
---@return boolean
function UtilsNs.holds_cycle(value) end

-- Reports true for dense Lua arrays starting at 1.
---@param value any # Value to inspect for array shape.
---@return boolean
function UtilsNs.is_array(value) end
