---@module "plugins._.sugar"

--- Plain constructors over routes, documents, and patches.
local sugar = {}

local is = require("plugins._.is")


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

---@param name string Alias name.
---@param value string Alias expansion.
---@return RcEntry Alias entry table.
function sugar.alias(name, value)
	return { alias = { name = name, expansion = value } }
end

---@param name string Variable name.
---@param value string Value under export.
---@return RcEntry Env entry table.
function sugar.env(name, value)
	return { env = { name = name, value = value } }
end

---@param dir Route Directory route under prepending to PATH.
---@return RcEntry PATH entry table.
function sugar.prepend(dir)
	return { path = { name = "PATH", dir = dir } }
end

---@param var string Variable name under prepending.
---@param dir Route Directory route under prepending.
---@return RcEntry Variable entry table.
function sugar.prepend_to(var, dir)
	return { path = { name = var, dir = dir } }
end

---@param argv Arg[] Eval command.
---@return RcEntry Eval entry table.
function sugar.eval(argv)
	return { eval = { argv = argv } }
end

---@param argv Arg[] Plain command.
---@return RcEntry Cmd entry table.
function sugar.cmd(argv)
	return { cmd = { argv = argv } }
end

---@param path Route File route for sourcing.
---@return RcEntry Source entry table.
function sugar.source(path)
	return { source = { path = path } }
end

return sugar
