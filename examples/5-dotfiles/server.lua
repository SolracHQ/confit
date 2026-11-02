local sugar = require("plugins._.sugar")

local installer = require("tools.mise")
local shell = require("tools.shell")
local ripgrep = require("tools.ripgrep")

return {
	configs = { installer, shell, ripgrep },
	documents = {
		sugar.rc(sugar.home(".bashrc")),
	},
}
