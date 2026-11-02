local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")

---@return Config Bat config for the profile configs array.
return mise.package({
	name = "bat",
	rc = {
		sugar.alias("cat", "bat"),
	},
})
