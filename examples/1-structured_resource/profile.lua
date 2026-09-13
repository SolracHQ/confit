local starship = require("tools.starship")({ command_timeout = 10000 })
return {
  shells = { "bash" },
  configs = { starship },
}
