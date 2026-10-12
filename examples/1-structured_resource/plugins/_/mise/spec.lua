---@module "plugins._.mise.spec"

---@class MiseSpec The package spec holding the tool name and its alias map.
---@field name string The package and config name.
---@field version string? The pinned version defaulting to latest.
---@field bin string? The binary defaulting to name.
---@field aliases {[string]: string}? The alias map or nil.
---@field options {[string]: any}? The backend tool options or nil.
---@field rc_builder (fun(rc: MiseRc))? The builder receiving the collector or nil.

--- Reports whether one value holds a non-empty string.
---@param value any Value under checking.
---@return boolean True for non-empty strings alone.
local function is_nonempty_string(value)
	return type(value) == "string" and value ~= ""
end

--- Checks one options table holding backend tool options.
---@param options {[string]: any} Raw options under checking.
local function check_options(options)
	if type(options) ~= "table" then
		error("mise: field 'options' must be a table")
	end
	for key, value in pairs(options) do
		if not is_nonempty_string(key) then
			error("mise: field 'options' keys must be non-empty strings")
		end
		local field = "mise: field 'options." .. key .. "' must be a string, number, boolean, or array of those"
		local value_type = type(value)
		if value_type == "string" or value_type == "number" or value_type == "boolean" then
			-- Scalar option, nothing more to check.
		elseif value_type == "table" then
			local count = 0
			local max = 0
			for item_key, item in pairs(value) do
				if type(item_key) ~= "number" or item_key % 1 ~= 0 or item_key < 1 then
					error(field)
				end
				if item_key > max then
					max = item_key
				end
				count = count + 1
				local item_type = type(item)
				if item_type ~= "string" and item_type ~= "number" and item_type ~= "boolean" then
					error(field)
				end
			end
			if max ~= count then
				error(field)
			end
		else
			error(field)
		end
	end
end

--- Checks one package spec into its validated fields.
---@param spec MiseSpec Raw package table under checking.
---@return string Package and config name.
---@return string Pinned version defaulting to latest.
---@return string Binary defaulting to name.
---@return {[string]: string}? Alias map or nil.
---@return {[string]: any}? Backend options or nil.
---@return (fun(rc: MiseRc))? Rc builder or nil.
local function check_spec(spec)
	if type(spec) ~= "table" then
		error("mise: package opts must be a table")
	end
	local name = spec.name
	if not is_nonempty_string(name) then
		error("mise: field 'name' must be a non-empty string")
	end
	local version = spec.version or "latest"
	if not is_nonempty_string(version) then
		error("mise: field 'version' must be a non-empty string")
	end
	local bin = spec.bin or name
	if not is_nonempty_string(bin) then
		error("mise: field 'bin' must be a non-empty string")
	end
	local rc_builder = spec.rc_builder
	if rc_builder and type(rc_builder) ~= "function" then
		error("mise: field 'rc_builder' must be a function or nil")
	end
	local aliases = spec.aliases
	if aliases and type(aliases) ~= "table" then
		error("mise: field 'aliases' must be a table")
	end
	local options = spec.options
	if options then
		check_options(options)
	end
	for key, _ in pairs(spec) do
		if
			key ~= "name"
			and key ~= "version"
			and key ~= "bin"
			and key ~= "aliases"
			and key ~= "options"
			and key ~= "rc_builder"
		then
			error("mise: field 'opts' unknown field '" .. tostring(key) .. "'")
		end
	end
	return name, version, bin, aliases, options, rc_builder
end

return { check = check_spec, nonempty = is_nonempty_string }
