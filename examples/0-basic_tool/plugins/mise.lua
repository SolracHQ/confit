---@module "plugins/mise"

-- Tool name with its alias map.
---@alias MiseSpec { name: string, aliases: { [string]: string } }

local sugar = require("plugins/sugar")

-- Mise tool installer. Declares tool installs with shell aliases.
local mise = {}

---@return table # Installer running `mise install`.
function mise.init()
	return {
		name = "mise",
		hooks = {
			{ argv = { "mise", "install" }, timeout = "10m" },
		},
	}
end

---@param spec MiseSpec # Tool name with its alias map, e.g. `{ name = "bat", aliases = { cat = "bat" } }`.
---@return table # Tool install with shell aliases.
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
