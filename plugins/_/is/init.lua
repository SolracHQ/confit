---@module "plugins._.is"

--- Shared value checks.
local is = {}

---@param value any Value under checking.
---@return boolean True for strings alone.
function is.string(value)
	return type(value) == "string"
end

---@param value any Value under checking.
---@return boolean True for non-empty strings alone.
function is.nonempty(value)
	return type(value) == "string" and value ~= ""
end

---@param value any Value under checking.
---@return boolean True for numbers alone.
function is.num(value)
	return type(value) == "number"
end

---@param value any Value under checking.
---@return boolean True for booleans alone.
function is.bool(value)
	return type(value) == "boolean"
end

---@param value any Value under checking.
---@return boolean True for tables alone.
function is.tab(value)
	return type(value) == "table"
end

---@param value any Value under checking.
---@return boolean True for functions alone.
function is.fun(value)
	return type(value) == "function"
end

---@generic T
---@param pred fun(value: any): boolean Check failing false.
---@param value T Value under checking.
---@param message string Error text on failure.
---@return T Value on success.
function is.assert(pred, value, message)
	if not pred(value) then
		error(message)
	end
	return value
end

---@param prefix string Plugin name prefixing the error.
---@param checked table Table under checking.
---@param allowed {[string]: boolean} Allowed key set.
---@param name string Table name shown in the error.
function is.keys(prefix, checked, allowed, name)
	for key, _ in pairs(checked) do
		if not allowed[key] then
			error(prefix .. ": field '" .. name .. "' unknown field '" .. tostring(key) .. "'")
		end
	end
end

---@generic T
---@param prefix string Error prefix with the trailing separator.
---@param value T? Value under passing through.
---@param reason string? Failure reason or nil.
---@return T Value on success.
function is.must(prefix, value, reason)
	if value == nil then
		error(prefix .. reason)
	end
	return value
end

return is
