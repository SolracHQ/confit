local mise = require("plugins._.mise")

local installer = mise.init()
local bat = require("tools.bat")

return {
	shells = { "bash" },
	configs = { installer, bat },
}
