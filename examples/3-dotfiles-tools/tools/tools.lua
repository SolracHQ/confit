local mise = confit.plugin.solrachq.mise

local bat = mise.package("bat", function(rc)
	rc:alias("cat", "bat")
end)
bat:add_patch(mise.activate())

local eza = mise.package("eza", function(rc)
	rc:alias("ls", "eza --icons=auto --color=auto")
end)

local ripgrep = mise.package("ripgrep")

local zoxide = mise.package("zoxide", function(rc)
	rc:alias("cd", "z")
	rc:eval({ "zoxide", "init", "bash" })
end)

local starship = mise.package("starship", function(rc)
	rc:eval({ "starship", "init", "bash" }, { section = "final" })
end)
local starship_path = confit.path.config("starship.toml")
local starship_body = confit.resources.load_text("resources/starship.toml")
starship:add_document(confit.document.text(starship_path, starship_body))

local shell = confit.config("shell")
shell:add_patch(confit.patch.rc(function(data)
	data:add("config", confit.document.rc.alias("ll", "ls -l"))
	data:add("config", confit.document.rc.alias("la", "ll -a"))
end))

return { bat, eza, ripgrep, zoxide, starship, shell }
