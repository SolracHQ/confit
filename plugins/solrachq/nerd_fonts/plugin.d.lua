---@meta _
-- Editor stub for the solrachq.nerd_fonts default plugin. This file ships
-- beside plugin.lua for language servers; the loader reads plugin.lua only.
-- Global scope mirrors what the Rust loader does: the plugin lands on
-- confit.plugin.solrachq.nerd_fonts, so profiles see it transparently.

---@class NerdFontsNs
-- Font installer dialect namespace. `font` builds the config from a name
-- plus an optional version, landing the fonts under `fonts/{name}`
-- with its own fc-cache refresh hook.
local NerdFontsNs = {}

-- Declares one nerd font plus its refresh hook.
---@param name string # Nerd font name matching the release zip, e.g. "JetBrainsMono".
---@param version? string # Pinned release, e.g. "3.4.0". Omitted resolves the latest release.
---@return table # Config userdata for the profile configs array.
function NerdFontsNs.font(name, version) end

---@class SolrachqNs
-- Default plugin user table. Each field is one embedded plugin.
---@field nerd_fonts NerdFontsNs # Nerd fonts installer dialect namespace.
local SolrachqNs = {}

---@class PluginNs
---@field solrachq SolrachqNs # Default plugin user table.
-- Re-opened to attach the default user table; the loader fills it at runtime.

confit.plugin.solrachq.nerd_fonts = NerdFontsNs
