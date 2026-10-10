---@module "plugins._.sugar"

-- The route table shows its base and its relative path.
---@alias SugarRoute { base: string, relative: string }

-- Route builders create route tables. Each builder returns one
-- route table. The table shows its base and its relative path.
local sugar = {}

local function route(base, relative)
	return { base = base, relative = relative }
end

---@param relative string Path under the home directory, e.g. ".bashrc".
---@return SugarRoute # Route table with the home base.
function sugar.home(relative)
	return route("home", relative)
end

---@param relative string Path under the config directory, e.g. "bat/config".
---@return SugarRoute # Route table with the config base.
function sugar.config(relative)
	return route("config", relative)
end

---@param relative string Path under the data directory.
---@return SugarRoute # Route table with the data base.
function sugar.data(relative)
	return route("data", relative)
end

---@param relative string Path under the cache directory.
---@return SugarRoute # Route table with the cache base.
function sugar.cache(relative)
	return route("cache", relative)
end

---@param path string Absolute path kept as is.
---@return SugarRoute # Route table with the literal base.
function sugar.literal(path)
	return { base = "literal", relative = path }
end

return sugar
