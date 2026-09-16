-- Welcome to confit. This profile builds your shell setup.
-- Profiles return three things: shells to render for, documents
-- holding file contents, and configs grouping your changes.

-- A config groups your changes under one name. Start with one
-- per tool or area. Configs hold patches, which modify
-- documents.
local shell = confit.config("shell")

-- Documents hold the files confit writes. This one declares
-- your shell startup lines in three sections: profile lines
-- render first, config lines render after them, final lines
-- render last.
local base = confit.document.rc.new({
  profile = {
    confit.document.rc.prepend(confit.path.home(".local/bin")),
  },
  config = {},
  final = {
    confit.document.rc.eval({ "starship", "init", "bash" }),
  },
})

-- Patches change documents. This one adds an alias to the
-- config section. Add more adds here, or more patches for
-- more tools. Run `confit plan` to preview, `confit apply`
-- to write.
shell:add_patch(confit.patch.rc(function(data)
  data:add("config", confit.document.rc.alias("ll", "ls -l"))
end))

return {
  shells = { "bash" },
  documents = { base },
  configs = { shell },
}
