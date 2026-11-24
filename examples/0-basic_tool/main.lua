local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")

local installer = mise.init("2026.10.8")
local bat = require("tools.bat")

return {
	configs = { installer, bat },
	documents = {
		sugar.rc(sugar.home(".bashrc")),
		sugar.rc(sugar.home(".zshrc")),
	},
}
