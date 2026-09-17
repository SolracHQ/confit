local mise = confit.plugin.solrachq.mise

local eza = mise.package({
	name = "eza",
	aliases = { ls = "eza --icons=auto --color=auto" },
})
return eza
