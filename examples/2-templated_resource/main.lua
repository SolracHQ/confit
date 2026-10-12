local mise = require("plugins._.mise")
local build_starship = require("tools.starship")

local installer = mise.init("2026.9.10")
local starship = build_starship({ timeout = 10000, palette = "catppuccin" })

return {
	configs = { installer, starship },
}
