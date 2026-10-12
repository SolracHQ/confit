local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")

local installer = mise.init("2026.9.10")
local starship = require("tools.starship")({ command_timeout = 10000 })

return {
	configs = { installer, starship },
	documents = {
		[sugar.home(".bashrc")] = sugar.rc(),
		[sugar.home(".zshrc")] = sugar.rc(),
	},
}
