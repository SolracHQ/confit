local mise_package = confit.plugin.solrachq.mise_package

local starship = mise_package("starship", function(rc)
	rc:alias("s", "starship")
	rc:init({ eval = { "starship", "init", "bash" } })
end)

return function(user_config)
	local artifact = confit.artifact.template(
		confit.path.config("starship.toml"),
		{ src = "resources/starship.toml.j2", vars = user_config }
	)
	starship:add_artifact(artifact)
	return starship
end
