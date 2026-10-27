-- Welcome to confit. This profile prepared your shell setup.
-- Profiles return configs and documents. Documents describe
-- file contents, configs group your changes.
local sugar = require("plugins._.sugar")

local bashrc = sugar.home(".bashrc")

-- A config groups your changes under one name. Start with one
-- per tool or area. Configs hold patches, which modify
-- documents. Run `confit plan` to preview, `confit apply`
-- to write.
local shell = {
	name = id("shell"),
	patches = {
		sugar.patch(bashrc, function(document)
			table.insert(document.config, sugar.alias("ll", "ls -l"))
		end),
	},
}

return {
	documents = {
		sugar.rc(bashrc),
	},
	configs = { shell },
}
