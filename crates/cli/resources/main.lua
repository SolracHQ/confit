-- Welcome to confit. This profile builds your shell setup.
-- Profiles return configs plus documents. Documents describe
-- file contents, configs group your changes.
local sugar = require("plugins._.sugar")

local bashrc = sugar.home(".bashrc")

-- A config groups your changes under one name. Start with one
-- per tool or area. Configs hold patches, which modify
-- documents. Run `confit plan` to preview, `confit apply`
-- to write.
local shell = {
	name = "shell",
	patches = {
		sugar.patch(bashrc, function(_, data)
			sugar.alias(data, "config", "ll", "ls -l")
		end),
	},
}

return {
	documents = {
		[bashrc] = sugar.rc(),
	},
	configs = { shell },
}
