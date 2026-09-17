local installer = require("tools.mise")
local bat = require("tools.bat")
local eza = require("tools.eza")
local ripgrep = require("tools.ripgrep")
local zoxide = require("tools.zoxide")
local starship = require("tools.starship")
local shell = require("tools.shell")
local fonts = require("tools.fonts")

return {
	shells = { "bash" },
	configs = { installer, bat, eza, ripgrep, zoxide, starship, shell, fonts },
}
