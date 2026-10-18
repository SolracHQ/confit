local mise = require("plugins._.mise")
local sugar = require("plugins._.sugar")

local installer = require("tools.mise")
local bat = require("tools.bat")
local eza = require("tools.eza")
local ripgrep = require("tools.ripgrep")
local zoxide = require("tools.zoxide")
local starship = require("tools.starship")
local shell = require("tools.shell")
local fonts = require("tools.fonts")

return {
	configs = { installer, bat, eza, ripgrep, zoxide, starship, shell, fonts },
	documents = {
		sugar.rc(sugar.home(".bashrc")),
		sugar.rc(sugar.home(".zshrc")),
	},
}
