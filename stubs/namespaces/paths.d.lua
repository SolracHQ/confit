---@meta _
-- Editor stubs for confit.path. The loader joins base folders with segments at runtime.
-- Shared shapes live in stubs/confit.d.lua.

---@class PathLib
-- Pure path helpers namespace.
local PathLib = {}

-- Joins $HOME with the segments.
---@param ... string # Path segments.
---@return string
function PathLib.home(...) end

-- Joins the OS config folder with the segments. Where managed files land.
---@param ... string # Path segments.
---@return string
function PathLib.config(...) end

-- Joins the OS data folder with the segments. Serves local installs like fonts.
---@param ... string # Path segments.
---@return string
function PathLib.data(...) end

-- Joins the confit project root with the segments. Serves symlinks to shipped resources.
---@param ... string # Path segments.
---@return string
function PathLib.confroot(...) end
