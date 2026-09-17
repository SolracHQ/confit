local mise = confit.plugin.solrachq.mise

local installer = mise.init("2026.9.10")
local bat = require("tools.bat")
return {
  shells = { "bash" },
  configs = { installer, bat },
}
