local starship = require("tools.starship")({ timeout = 10000, palette = "catppuccin" })
return {
  shells = { "bash" },
  configs = { starship },
}
