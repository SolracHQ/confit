local render = require("confit.render")

--- Build one theme config from the answered name.
---@param name string
---@return Config
return function(name)
	local text = assert(render("theme = \"{{ theme }}\"\n", { theme = name }))
	return {
		name = id("theme"),
		documents = {
			{
				path = route("config", "theme/name.txt"),
				text = text,
			},
		},
	}
end
