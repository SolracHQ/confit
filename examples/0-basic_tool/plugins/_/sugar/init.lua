---@module "plugins._.sugar"

--- Plain constructors over routes, documents, and patches.
local sugar = {}


local function route(base, relative)
	return { base = base, relative = relative }
end

---@param relative string Path under the home directory, e.g. ".bashrc".
---@return Route Route table with the home base.
function sugar.home(relative)
	return route("home", relative)
end

---@param relative string Path under the config directory, e.g. "bat/config".
---@return Route Route table with the config base.
function sugar.config(relative)
	return route("config", relative)
end

---@param relative string Path under the data directory.
---@return Route Route table with the data base.
function sugar.data(relative)
	return route("data", relative)
end

---@param relative string Path under the cache directory.
---@return Route Route table with the cache base.
function sugar.cache(relative)
	return route("cache", relative)
end

---@param path string Absolute path kept as is.
---@return Route Route table with the literal base.
function sugar.literal(path)
	return { base = "literal", relative = path }
end

---@return table Empty rc sections for shell documents.
function sugar.rc()
	return { profile = {}, config = {}, final = {} }
end

---@param target Route Document route under patching.
---@param apply fun(route: Route, data: table) Callback receiving route beside live data.
---@return table Patch table holding target plus apply.
function sugar.patch(target, apply)
	return { target = target, apply = apply }
end

---@param data table Live shell sections under patching.
---@param section string Section receiving the entry.
---@param name string Alias name.
---@param expansion string Alias expansion.
function sugar.alias(data, section, name, expansion)
	table.insert(data[section], { alias = { name = name, expansion = expansion } })
end

return sugar
