local mise = confit.plugin.solrachq.mise

local installer = mise.init("2026.9.10")
local starship = require("tools.starship")({ timeout = 10000, palette = "catppuccin" })
return {
  shells = { "bash" },
  configs = { installer, starship },
}
