local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")

---@return Config
return mise.package({
	name = "zoxide",
	rc = {
		sugar.alias("cd", "z"),
		final = {
			sugar.eval({ "zoxide", "init", "bash" }),
		},
	},
})
