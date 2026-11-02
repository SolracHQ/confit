local render = require("confit.render")

--- Build one theme config from the answered name.
---@param name string The theme name from input().
---@return Config Theme config for the profile configs array.
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
