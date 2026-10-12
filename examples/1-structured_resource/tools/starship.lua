--- Builds the starship config over the base TOML resource.
local mise = require("plugins._.mise")
local resource = require("confit.resource")

---@type Route
local STARSHIP_TOML = { base = "config", relative = "starship.toml" }

---@param user_config table User keys winning over the base data.
---@return Config Starship config for the profile configs array.
return function(user_config)
	local base = assert(assert(resource("resources/starship.toml")):load())
	local config = mise.package({
		name = "starship",
		aliases = { s = "starship" },
		rc_builder = function(rc)
			rc:eval({ "starship", "init", "bash" })
		end,
	})
	config.documents = { [STARSHIP_TOML] = { format = "toml", data = base } }
	table.insert(config.patches, {
		target = STARSHIP_TOML,
		format = "toml",
		apply = function(_, data)
			for key, value in pairs(user_config) do
				data.data[key] = value
			end
		end,
	})
	return config
end
