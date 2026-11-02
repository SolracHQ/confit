local sugar = require("plugins._.sugar")

local installer = require("tools.mise")
local shell = require("tools.shell")
local bat = require("tools.bat")
local eza = require("tools.eza")
local zoxide = require("tools.zoxide")
local ripgrep = require("tools.ripgrep")
local starship = require("tools.starship")({ palette = "catppuccin", add_newline = false })
local fonts = require("tools.fonts")
local theme = require("plugins.solrac.theme")("catppuccin")

return {
	configs = { installer, shell, bat, eza, zoxide, ripgrep, starship, fonts, theme },
	documents = {
		sugar.rc(sugar.home(".bashrc")),
		sugar.rc(sugar.home(".zshrc")),
	},
}
