local mise = confit.plugin.solrachq.mise

local starship = mise.package({
	name = "starship",
	rc_builder = function(rc)
		rc:alias("s", "starship")
		rc:eval({ "starship", "init", "bash" })
	end,
})

return function(user_config)
	local path = confit.path.config("starship.toml")
	local base = confit.fetch("resources/starship.toml"):toml()
	starship:add_document(confit.document.structured("toml", {
		path = path,
		data = base,
	}))
	starship:add_patch(confit.patch.structured("toml", path, function(data)
		for key, value in pairs(user_config) do
			data:set(key, value)
		end
	end))
	return starship
end
