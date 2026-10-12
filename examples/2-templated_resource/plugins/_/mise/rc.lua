---@module "plugins._.mise.rc"

local spec = require("plugins._.mise.spec")
local is_nonempty_string = spec.nonempty

---@class MiseRcOpts The caller opts holding the patch section and the entry guard.
---@field section ("profile"|"config"|"final")? The patch section defaulting to config.
---@field when Condition? The caller guard winning over the binary guard.

---@class MiseRc The rc collector type flowing into builder callbacks.
---@field alias fun(self: MiseRc, name: string, value: string, opts: MiseRcOpts?) The alias entry.
---@field env fun(self: MiseRc, name: string, value: string, opts: MiseRcOpts?) The env entry.
---@field prepend fun(self: MiseRc, dir: Route, opts: MiseRcOpts?) The PATH entry.
---@field prepend_to fun(self: MiseRc, var: string, dir: Route, opts: MiseRcOpts?) The variable entry.
---@field eval fun(self: MiseRc, argv: Arg[], opts: MiseRcOpts?) The eval entry.
---@field cmd fun(self: MiseRc, argv: Arg[], opts: MiseRcOpts?) The cmd entry.
---@field source fun(self: MiseRc, path: Route, opts: MiseRcOpts?) The source entry.

--- Home dot-rc routes carry shell documents; mise never declares one.
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
		error("mise: route '" .. tostring(route.relative) .. "' holds no shell name")
	end
	return shell
end

--- Splits caller opts into the patch section and the entry guard.
---@param guard Condition Automatic in_path guard for the binary.
---@param opts MiseRcOpts? Caller opts holding section and when.
---@return string Section name defaulting to config.
---@return Condition Guard unless caller when wins.
local function section_and_guard(guard, opts)
	local section = "config"
	local when = guard
	if opts then
		if type(opts) ~= "table" then
			error("mise: rc opts must be a table")
		end
		for key, value in pairs(opts) do
			if key == "section" then
				section = value
			elseif key == "when" then
				when = value
			else
				error("mise: field 'opts' unknown field '" .. tostring(key) .. "'")
			end
		end
	end
	if section ~= "profile" and section ~= "config" and section ~= "final" then
		error("mise: field 'section' must be one of 'profile', 'config', 'final'")
	end
	return section, when
end

--- Builds the rc collector and the flush closing one callback run.
---@param binary string Binary guarding every collected entry.
---@return MiseRc Collector receiving alias, env, prepend, eval, cmd, source.
---@return (fun(): RcPatch?) Flush returning one filter patch or nil when empty.
local function collector(binary)
	---@type Condition
	local guard = { in_path = { name = binary } }
	local pending = {}
	---@param section string Patch section receiving the entry.
	---@param entry RcEntry Entry landing in the section.
	local function stash(section, entry)
		table.insert(pending, { section, entry })
	end
	local rc = {}
	---@param name string Alias name.
	---@param value string Alias expansion.
	---@param opts MiseRcOpts? Caller opts holding section and when.
	function rc:alias(name, value, opts)
		if not is_nonempty_string(name) then
			error("mise: field 'name' must be a non-empty string")
		end
		if not is_nonempty_string(value) then
			error("mise: field 'expansion' must be a non-empty string")
		end
		local section, when = section_and_guard(guard, opts)
		stash(section, { alias = { name = name, expansion = value, when = when } })
	end
	---@param name string Variable name.
	---@param value string Value under export.
	---@param opts MiseRcOpts? Caller opts holding section and when.
	function rc:env(name, value, opts)
		if not is_nonempty_string(name) then
			error("mise: field 'name' must be a non-empty string")
		end
		if type(value) ~= "string" then
			error("mise: field 'value' must be a string")
		end
		local section, when = section_and_guard(guard, opts)
		stash(section, { env = { name = name, value = value, when = when } })
	end
	---@param dir Route Directory route under prepending to PATH.
	---@param opts MiseRcOpts? Caller opts holding section and when.
	function rc:prepend(dir, opts)
		if type(dir) ~= "table" then
			error("mise: field 'dir' must be a route table")
		end
		local section, when = section_and_guard(guard, opts)
		stash(section, { path = { name = "PATH", dir = dir, when = when } })
	end
	---@param var string Variable name under prepending.
	---@param dir Route Directory route under prepending.
	---@param opts MiseRcOpts? Caller opts holding section and when.
	function rc:prepend_to(var, dir, opts)
		if not is_nonempty_string(var) then
			error("mise: field 'var' must be a non-empty string")
		end
		if type(dir) ~= "table" then
			error("mise: field 'dir' must be a route table")
		end
		local section, when = section_and_guard(guard, opts)
		stash(section, { path = { name = var, dir = dir, when = when } })
	end
	---@param argv Arg[] Eval command.
	---@param opts MiseRcOpts? Caller opts holding section and when.
	function rc:eval(argv, opts)
		if type(argv) ~= "table" then
			error("mise: field 'argv' must be an array")
		end
		local section, when = section_and_guard(guard, opts)
		stash(section, { eval = { argv = argv, when = when } })
	end
	---@param argv Arg[] Plain command.
	---@param opts MiseRcOpts? Caller opts holding section and when.
	function rc:cmd(argv, opts)
		if type(argv) ~= "table" then
			error("mise: field 'argv' must be an array")
		end
		local section, when = section_and_guard(guard, opts)
		stash(section, { cmd = { argv = argv, when = when } })
	end
	---@param path Route File route for sourcing.
	---@param opts MiseRcOpts? Caller opts holding section and when.
	function rc:source(path, opts)
		if type(path) ~= "table" then
			error("mise: field 'path' must be a route table")
		end
		local section, when = section_and_guard(guard, opts)
		stash(section, { source = { path = path, when = when } })
	end
	---@return RcPatch? One filter patch or nil when empty.
	local function flush()
		if #pending == 0 then
			return nil
		end
		return {
			filter = is_shell,
			---@param data RcData Live shell sections under patching.
			apply = function(_, data)
				for _, item in ipairs(pending) do
					local section = item[1]
					table.insert(data[section], item[2])
				end
			end,
		}
	end
	return rc, flush
end

--- Builds the rc filter patches for one package binary.
---@param bin string Binary guarding every entry.
---@param rc_builder (fun(rc: MiseRc))? Builder receiving the collector or nil.
---@param aliases {[string]: string}? Alias map or nil.
---@return RcPatch[] Filter patches in builder then alias order.
local function rc_patches(bin, rc_builder, aliases)
	local patches = {}
	if rc_builder then
		local rc, flush = collector(bin)
		rc_builder(rc)
		local patch = flush()
		if patch then
			table.insert(patches, patch)
		end
	end
	if aliases then
		local rc, flush = collector(bin)
		for alias_name, expansion in pairs(aliases) do
			if not is_nonempty_string(alias_name) then
				error("mise: field 'aliases' keys must be non-empty strings")
			end
			if not is_nonempty_string(expansion) then
				error("mise: field 'aliases' values must be non-empty strings")
			end
			rc:alias(alias_name, expansion)
		end
		local patch = flush()
		if patch then
			table.insert(patches, patch)
		end
	end
	return patches
end

return { rc_patches = rc_patches, is_shell = is_shell, shell_of = shell_of }
