---@module "plugins._.mise"

-- The spec gives the tool name and its alias map.
---@alias MiseSpec { name: string, aliases: { [string]: string } }

local sugar = require("plugins._.sugar")

-- The module installs mise tools. It declares tool installs and shell aliases.
local mise = {}

---@return table # The installer runs `mise install`.
function mise.init()
	return {
		name = "mise",
		hooks = {
			{ argv = { "mise", "install" }, timeout = "10m" },
		},
	}
end

---@param spec MiseSpec # The spec gives the tool name and its alias map, e.g. `{ name = "bat", aliases = { cat = "bat" } }`.
---@return table # The table describes the tool install and shell aliases.
function mise.package(spec)
	local entries = {}
	for name, expansion in pairs(spec.aliases or {}) do
		entries[#entries + 1] = { alias = { name = name, expansion = expansion } }
	end
	return {
		name = "tools/" .. spec.name,
		documents = {
			[sugar.home(".bashrc")] = { config = entries },
		},
		hooks = {
			{ argv = { "mise", "install", spec.name }, timeout = "10m" },
		},
	}
end

return mise
