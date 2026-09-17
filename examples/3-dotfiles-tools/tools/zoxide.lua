local mise = confit.plugin.solrachq.mise

local zoxide = mise.package({
	name = "zoxide",
	rc_builder = function(rc)
		rc:alias("cd", "z")
		rc:eval({ "zoxide", "init", "bash" })
	end,
})
return zoxide
