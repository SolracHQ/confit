---@module "plugins._.mise.rc"

local is = require("plugins._.is")
local tab = require("plugins._.tab")

--- Home dot-rc routes carry shell documents.
local SHELL_PATTERN = "^%.(%a+)rc$"

--- Reports whether one route holds a shell document.
---@param route Route Destination route with base and relative.
---@return boolean True for home dot-rc routes alone.
local function is_shell(route)
	return route.base == "home" and route.relative:match(SHELL_PATTERN) ~= nil
end

--- Derives the shell name from one shell route.
---@param route Route Shell route under reading.
---@return string Shell name without dots and rc.
local function shell_of(route)
	local shell = route.relative:match(SHELL_PATTERN)
	if not shell then
		error("mise: route '" .. route.relative .. "' holds no shell name")
	end
	return shell
end

---@param member RcEntry_Env|RcEntry_Path|RcEntry_Alias|RcEntry_Eval|RcEntry_Cmd|RcEntry_Source Union member under guarding.
---@param guard Condition Default entry guard.
---@return table Member copy with the default guard.
local function with_guard(member, guard)
	local guarded = tab.clone(member)
	if guarded.when == nil then
		guarded.when = guard
	else
		guarded.when = { all = { guard, guarded.when } }
	end
	return guarded
end

---@param entry RcEntry Rc entry under stamping.
---@param guard Condition Default entry guard.
---@return table Stamped entry copy with default guards.
local function stamp(entry, guard)
	local stamped = {}
	for union, member in pairs(entry) do
		stamped[union] = is.tab(member) and with_guard(member, guard) or member
	end
	return stamped
end

--- Builds the rc filter patches for one package binary.
---@param bin string Binary guarding every entry.
---@param rc MiseRcSections? Section rc entries or nil.
---@return RcPatch[] One filter patch or none when empty.
local function rc_patches(bin, rc)
	if rc == nil then
		return {}
	end
	local guard = { in_path = { name = bin } }
	local items = {}
	for _, section in ipairs({ "profile", "config", "final" }) do
		for _, item in ipairs(rc[section] or {}) do
			table.insert(items, { section, stamp(item, guard) })
		end
	end
	if #items == 0 then
		return {}
	end
	return {
		{
			filter = is_shell,
			---@param data RcData Live shell sections under patching.
			apply = function(_, data)
				for _, item in ipairs(items) do
					table.insert(data[item[1]], item[2])
				end
			end,
		},
	}
end

return { rc_patches = rc_patches, is_shell = is_shell, shell_of = shell_of }
