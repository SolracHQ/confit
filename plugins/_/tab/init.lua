---@module "plugins._.tab"

--- Table utilities.
local tab = {}

local is = require("plugins._.is")

---@param source table Table under cloning.
---@return table Shallow copy.
function tab.clone(source)
	is.assert(is.tab, source, "tab: source must be a table")
	local copy = {}
	for key, value in pairs(source) do
		copy[key] = value
	end
	return copy
end

return tab
