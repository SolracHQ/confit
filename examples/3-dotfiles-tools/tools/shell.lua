local shell = confit.config("shell")
shell:add_patch(confit.patch.rc(function(data)
	data:add("config", confit.document.rc.alias("ll", "ls -l"))
	data:add("config", confit.document.rc.alias("la", "ll -a"))
end))
return shell
