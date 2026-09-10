local bat = confit.tool("bat", {
	install = confit.mise.package({ name = "bat" }),
})
bat:alias("cat", "bat")
return bat
