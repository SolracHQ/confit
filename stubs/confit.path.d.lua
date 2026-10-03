--- @meta confit.path

--- Path value for late-bound destinations.
--- @class PathHandle
local _CLASS_PathHandle_ = {
	--- Reads the destination base name.
	--- @param self PathHandle
	--- @return string
	base = function(self) end,
	--- Reads the portable route display.
	--- @param self PathHandle
	--- @return string
	display = function(self) end,
	--- Reads the relative path text.
	--- @param self PathHandle
	--- @return string
	relative = function(self) end,
}

--- Joins the user home folder with segments.
--- @param ... string Path segments.
--- @return PathHandle #1: The joined path value.
function home(...) end

--- Joins the platform config folder with segments.
--- @param ... string Path segments.
--- @return PathHandle #1: The joined path value.
function config(...) end

--- Joins the platform data folder with segments.
--- @param ... string Path segments.
--- @return PathHandle #1: The joined path value.
function data(...) end

--- Joins the platform cache folder with segments.
--- @param ... string Path segments.
--- @return PathHandle #1: The joined path value.
function cache(...) end

--- Carries a host-specific path verbatim.
--- @param ... string Path segments.
--- @return PathHandle #1: The literal path value.
function literal(...) end


return {home = home, config = config, data = data, cache = cache, literal = literal}
