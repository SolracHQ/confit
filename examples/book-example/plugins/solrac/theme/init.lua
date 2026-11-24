---@module "plugins.solrac.theme"

local sugar = require("plugins._.sugar")
local open = require("confit.open")
local render = require("confit.render")
local validate = require("confit.validate")

--- Bat config template.
local TEMPLATE = "resources/bat.conf.j2"

--- Home dot-rc routes carry shell documents.
local SHELL_PATTERN = "^%.(%a+)rc$"

---@type Route
local BAT_CONFIG = sugar.config("bat/config")

--- Builds the bat theme config from the template.
---@param name string
---@return Config
return function(name)
	local template = assert(assert(open(TEMPLATE)):text())
	local text = assert(render(template, { theme = name }))
	local config = {
		name = id("theme"),
		documents = {
			{ path = BAT_CONFIG, text = text },
		},
		patches = {
			{
				filter = function(route)
					return route.base == "home" and route.relative:match(SHELL_PATTERN) ~= nil
				end,
				---@param document Document_Rc
				apply = function(document)
					table.insert(document.config, sugar.env("BAT_THEME", name))
				end,
			},
		},
	}
	local checked, reason = validate.config(config)
	if not checked then
		error("theme: invalid config: " .. tostring(reason))
	end
	return checked
end
