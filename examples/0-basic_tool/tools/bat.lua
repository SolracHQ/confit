local mise_package = confit.plugin.solrachq.mise_package

local bat = mise_package("bat", function(rc)
	rc:alias("cat", "bat")
end)
return bat
