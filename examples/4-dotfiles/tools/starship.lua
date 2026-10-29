local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")
local open = require("confit.open")

---@type Route
local STARSHIP_TOML = route("config", "starship.toml")

local base = assert(assert(open("resources/starship.toml")):load())
local config = mise.package({
	name = "starship",
	rc = {
		final = {
			sugar.eval({ "starship", "init", "bash" }),
		},
	},
})
config.documents = { { path = STARSHIP_TOML, format = "toml", data = base } }

---@return Config Starship config for the profile configs array.
return config
