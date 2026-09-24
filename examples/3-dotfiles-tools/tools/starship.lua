local mise = confit.plugin.solrachq.mise

local starship = mise.package({
	name = "starship",
	rc_builder = function(rc)
		rc:eval({ "starship", "init", "bash" }, { section = "final" })
	end,
})
local starship_path = confit.path.config("starship.toml")
local starship_body = confit.fetch("resources/starship.toml"):text()
starship:add_document(confit.document.text(starship_path, starship_body))
return starship
