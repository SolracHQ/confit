---@module "plugins._.mise.spec"

local is = require("plugins._.is")

---@class MiseRcSections Rc entries keyed by document section.
---@field profile RcEntry[]? Entries before the guard.
---@field config RcEntry[]? Entries after the guard.
---@field final RcEntry[]? Entries last.

---@class MiseSpec The package spec holding the tool name and its rc entries.
---@field name string The package and config name.
---@field version string? The pinned version defaulting to latest.
---@field bin string? The binary defaulting to name.
---@field options {[string]: any}? The backend tool options or nil.
---@field rc (RcEntry[]|MiseRcSections)? Positional entries read config with named sections overriding or nil.

--- Checks one options table holding backend tool options.
---@param options {[string]: any} Raw options under checking.
local function check_options(options)
	is.assert(is.tab, options, "mise: field 'options' must be a table")
	for key, value in pairs(options) do
		is.assert(is.nonempty, key, "mise: field 'options' keys must be non-empty strings")
		local field = "mise: field 'options." .. key .. "' must be a string, number, boolean, or array of those"
		if is.string(value) or is.num(value) or is.bool(value) then
			-- Scalar option, nothing more to check.
		elseif is.tab(value) then
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

--- Normalizes one rc table into section entries.
---@param rc (RcEntry[]|MiseRcSections) Raw rc table under normalizing.
---@return MiseRcSections Section entries in profile, config, final order.
local function normalize_rc(rc)
	local normalized = {}
	local count = 0
	for _, item in ipairs(rc) do
		count = count + 1
		normalized.config = normalized.config or {}
		table.insert(normalized.config, item)
	end
	for _, section in ipairs({ "profile", "config", "final" }) do
		if rc[section] then
			is.assert(is.tab, rc[section], "mise: field 'rc." .. section .. "' must be an array")
			normalized[section] = normalized[section] or {}
			for _, item in ipairs(rc[section]) do
				table.insert(normalized[section], item)
			end
		end
	end
	for key, _ in pairs(rc) do
		local known = key == "profile" or key == "config" or key == "final"
		local seen = type(key) == "number" and key >= 1 and key % 1 == 0 and key <= count
		if not known and not seen then
			error("mise: field 'rc' unknown field '" .. tostring(key) .. "'")
		end
	end
	return normalized
end

--- Checks one package spec into its validated fields.
---@param spec MiseSpec Raw package table under checking.
---@return string Package and config name.
---@return string Pinned version defaulting to latest.
---@return string Binary defaulting to name.
---@return {[string]: any}? Backend options or nil.
---@return MiseRcSections? Section rc entries or nil.
local function check_spec(spec)
	is.assert(is.tab, spec, "mise: package opts must be a table")
	local name = is.assert(is.nonempty, spec.name, "mise: field 'name' must be a non-empty string")
	local version = is.assert(is.nonempty, spec.version or "latest", "mise: field 'version' must be a non-empty string")
	local bin = is.assert(is.nonempty, spec.bin or name, "mise: field 'bin' must be a non-empty string")
	local rc = spec.rc
	if rc then
		is.assert(is.tab, rc, "mise: field 'rc' must be a table or nil")
		rc = normalize_rc(rc)
	end
	local options = spec.options
	if options then
		check_options(options)
	end
	is.keys("mise", spec, {
		name = true,
		version = true,
		bin = true,
		options = true,
		rc = true,
	}, "opts")
	return name, version, bin, options, rc
end

return { check = check_spec }
