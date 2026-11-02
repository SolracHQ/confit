local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")

--- Home dot-rc routes carry shell documents.
local SHELL_PATTERN = "^%.(%a+)rc$"

local config = mise.package({ name = "zoxide" })

table.insert(config.patches, {
	filter = function(route)
		return route.base == "home" and route.relative:match(SHELL_PATTERN) ~= nil
	end,
	---@param document Document_Rc
	apply = function(document)
		local shell = document.path.relative:match(SHELL_PATTERN)
		if not shell then
			error("zoxide: route '" .. document.path.relative .. "' holds no shell name")
		end
		table.insert(document.config, sugar.alias("cd", "z"))
		table.insert(document.final, sugar.eval({ "zoxide", "init", shell }))
	end,
})

---@return Config
return config
