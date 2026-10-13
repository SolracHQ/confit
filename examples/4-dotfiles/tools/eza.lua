local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")

---@return Config Eza config for the profile configs array.
return mise.package({
	name = "eza",
	rc = {
		sugar.alias("ls", "eza --icons=auto --color=auto"),
	},
})
