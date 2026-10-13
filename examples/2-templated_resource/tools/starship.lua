--- Renders starship TOML from template vars with shell entries.
local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")
local resource = require("confit.resource")
local render = require("confit.template")

local TEMPLATE = "resources/starship.toml.j2"

---@type Route
local STARSHIP_TOML = { base = "config", relative = "starship.toml" }

---@param opts table User vars for the template.
---@return Config Starship config for the profile configs array.
return function(opts)
	local text = assert(assert(resource(TEMPLATE)):text())
	local rendered = assert(render(text, opts))
	local config = mise.package({
		name = "starship",
		rc = {
			sugar.alias("s", "starship"),
			final = {
				sugar.eval({ "starship", "init", "bash" }),
			},
		},
	})
	config.documents = { [STARSHIP_TOML] = { text = rendered } }
	return config
end
