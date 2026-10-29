--- Builds the starship config over the base TOML resource.
local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")
local open = require("confit.open")

---@type Route
local STARSHIP_TOML = route("config", "starship.toml")

---@param user_config table User keys winning over the base data.
---@return Config Starship config for the profile configs array.
return function(user_config)
	local base = assert(assert(open("resources/starship.toml")):load())
	local config = mise.package({
		name = "starship",
		rc = {
			sugar.alias("s", "starship"),
			final = {
				sugar.eval({ "starship", "init", "bash" }),
			},
		},
	})
	config.documents = { { path = STARSHIP_TOML, format = "toml", data = base } }
	table.insert(config.patches, {
		target = STARSHIP_TOML,
		format = "toml",
		---@param document Document_Structured Live TOML document under patching.
		apply = function(document)
			for key, value in pairs(user_config) do
				document.data[key] = value
			end
		end,
	})
	return config
end
