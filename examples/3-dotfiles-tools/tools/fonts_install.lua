local nerd_fonts = confit.plugin.solrachq.nerd_fonts

-- Font installer holding the shared fc-cache hook. Fonts
-- require this target, so the refresh anchor stays declared.
return nerd_fonts.init()
