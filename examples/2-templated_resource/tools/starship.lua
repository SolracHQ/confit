local starship = confit.tool("starship", {
	install = confit.mise.package({ name = "starship" }),
})
starship:alias("s", "starship")
starship:init({ eval = { "starship", "init", "bash" } })

return function(user_config)
	local artifact = confit.artifact.template(
		confit.path.config("starship.toml"),
		{ src = "resources/starship.toml.j2", vars = user_config }
	)
	starship:append_artifact(artifact)
	return starship
end
