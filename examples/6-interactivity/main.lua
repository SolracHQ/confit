local input = require("confit.input")
local build_theme = require("tools.theme")

local answer = input("Theme name: ")
if answer == "" then
	answer = "default"
end

return {
	configs = { build_theme(answer) },
}
