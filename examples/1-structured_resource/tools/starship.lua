local starship = confit.tool("starship", {
	install = confit.mise.package({ name = "starship" }),
})
starship:alias("s", "starship")
starship:init({ eval = { "starship", "init", "bash" } })

return function(user_config)
	local resource = confit.resources.load_toml("resources/starship.toml")
	local config = confit.resources.merge(resource, user_config)
	local artifact = confit.artifact.toml(
		confit.path.config("starship.toml"),
		config
	)
	starship:append_artifact(artifact)
	return starship
end
