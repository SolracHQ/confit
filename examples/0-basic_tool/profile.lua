local mise = require("plugins/mise")

local installer = mise.init()
local bat = require("tools/bat")

return {
	shells = { "bash" },
	configs = { installer, bat },
}
