---@meta _
-- Editor stub for the solrachq.template default plugin. This file ships
-- beside plugin.lua for language servers; the loader reads plugin.lua only.
-- Global scope mirrors what the Rust loader does: the plugin lands on
-- confit.plugin.solrachq.template, so profiles see it transparently.

---@class TemplateOpts
---@field src string # Root-relative template path, e.g. "resources/starship.toml.j2".
---@field vars table? # Template variables. Defaults to empty.
-- Options for the template plugin. Unknown keys are plan errors.
local TemplateOpts = {}

-- Renders one template file into a plain text document.
---@param path string # Destination path.
---@param opts TemplateOpts # Template source plus variables.
---@return table # Plain text document table for config:add_document.
local function template(path, opts) end

---@class SolrachqNs
-- Default plugin user table. Each field is one embedded plugin.
---@field template fun(path: string, opts: TemplateOpts): table # Renders one template file into a text document.
local Solrachq = {}

---@class PluginNs
---@field solrachq SolrachqNs # Default plugin user table.
-- Re-opened to attach the default user table; the loader fills it at runtime.

confit.plugin.solrachq.template = template
