local sugar = require("plugins._.sugar")

---@param route Route Destination route under reading.
---@return boolean True for the managed shell documents alone.
local function is_shell(route)
	return route.base == "home" and (route.relative == ".bashrc" or route.relative == ".zshrc")
end

---@return Config Shell config for the profile configs array.
return {
	name = id("shell"),
	patches = {
		{
			filter = is_shell,
			---@param document Document_Rc Live shell document under patching.
			apply = function(document)
				table.insert(document.config, sugar.alias("ll", "ls -l"))
				table.insert(document.config, sugar.alias("la", "ll -a"))
			end,
		},
	},
}
