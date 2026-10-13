local sugar = require("plugins._.sugar")

---@param route Route Destination route under reading.
---@return boolean True for the managed shell documents alone.
local function is_shell(route)
	return route.base == "home" and (route.relative == ".bashrc" or route.relative == ".zshrc")
end

---@return Config Shell config for the profile configs array.
return {
	name = "shell",
	patches = {
		{
			filter = is_shell,
			---@param data RcData Live shell sections under patching.
			apply = function(_, data)
				table.insert(data.config, sugar.alias("ll", "ls -l"))
				table.insert(data.config, sugar.alias("la", "ll -a"))
			end,
		},
	},
}
