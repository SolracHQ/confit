local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")

---@return Config
return mise.package({
	name = "bat",
	rc = {
		sugar.alias("cat", "bat"),
	},
})
