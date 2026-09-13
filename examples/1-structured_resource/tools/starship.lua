local mise_package = confit.plugin.solrachq.mise_package

local starship = mise_package("starship", function(rc)
	rc:alias("s", "starship")
	rc:init({ eval = { "starship", "init", "bash" } })
end)

return function(user_config)
	local resource = confit.resources.load_toml("resources/starship.toml")
	local config = confit.resources.merge(resource, user_config)
	local artifact = confit.artifact.toml(
		confit.path.config("starship.toml"),
		config
	)
	starship:add_artifact(artifact)
	return starship
end
