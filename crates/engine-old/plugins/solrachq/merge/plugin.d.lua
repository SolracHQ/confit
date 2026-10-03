---@meta _
-- Editor stub for the solrachq.merge default plugin. This file ships
-- beside plugin.lua for language servers; the loader reads plugin.lua only.
-- Global scope mirrors what the Rust loader does: the plugin lands on
-- confit.plugin.solrachq.merge, so profiles see it transparently.

---@class MergeOpts
---@field shallow boolean? # Merge top-level keys only. Defaults to false.
---@field list_append boolean? # Concatenate arrays instead of replacing. Defaults to false.
-- Options for the merge plugin. Unknown keys are plan errors.
local MergeOpts = {}

-- Deep-merges overlay over base into a fresh table. Tables recurse,
-- everything else last-wins.
---@param base table # Base table.
---@param overlay table # Overlay table, wins on overlap.
---@param opts MergeOpts? # Optional tweaks.
---@return table
local function merge(base, overlay, opts) end

---@class SolrachqNs
-- Default plugin user table. Each field is one embedded plugin.
---@field merge fun(base: table, overlay: table, opts?: MergeOpts): table # Deep-merges overlay over base.
local Solrachq = {}

---@class PluginNs
---@field solrachq SolrachqNs # Default plugin user table.
-- Re-opened to attach the default user table; the loader fills it at runtime.

confit.plugin.solrachq.merge = merge
