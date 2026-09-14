local mise = confit.plugin.solrachq.mise

local bat = mise.package("bat", function(rc)
	rc:alias("cat", "bat")
end)
bat:add_document(mise.activate())
return bat
