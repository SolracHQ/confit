local tools = require("tools.tools")

local font_url = "https://github.com/ryanoasis/nerd-fonts/releases/download/v3.5.1/JetBrainsMono.zip"
local archive = confit.resources.fetch_file(font_url)
local fonts = confit.document.compressed(archive, function(path, _, content)
	if path:match("%.ttf$") then
		local name = path:match("([^/]+)$")
		local dest = confit.path.data("fonts", name)
		return confit.document.opaque(dest, content)
	end
end)

return {
	shells = { "bash" },
	documents = fonts,
	configs = tools,
}
