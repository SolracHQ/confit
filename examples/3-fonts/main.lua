local fonts = require("plugins._.nerd_fonts")
local sugar = require("plugins._.sugar")

local jetbrains = fonts.font("JetBrainsMono", "3.5.1")

return {
	configs = { jetbrains },
	documents = {
		[sugar.home(".bashrc")] = sugar.rc(),
	},
}
