local mise = confit.plugin.solrachq.mise
local template = confit.plugin.solrachq.template

local starship = mise.package({
	name = "starship",
	rc_builder = function(rc)
		rc:alias("s", "starship")
		rc:eval({ "starship", "init", "bash" })
	end,
})

return function(user_config)
	local document = template(confit.path.config("starship.toml"), {
		src = "resources/starship.toml.j2",
		vars = user_config,
	})
	starship:add_document(document)
	return starship
end
