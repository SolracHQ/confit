-- solrachq.mise default plugin.
--
-- Installer dialect over config plus patch primitives:
-- `package(name, callback)` generates the config under the package name,
-- declares the mise install as a TOML document for the shared mise config
-- path, then runs the callback with an rc collector. Every callback entry
-- carries `when = confit.shell.in_path(binary)`, so shell lines stay inert
-- until the binary lands on PATH. `activate()` returns the patch adding
-- the eval entry for mise activation with the `{{shell}}` slot. Callers
-- attach it with `config:add_patch`.
--
-- Per-shell activation choice: the plugin runs during profile evaluation,
-- before the profile declares its shells, so it cannot name a shell here.
-- The activation argv carries the `{{shell}}` template slot (see
-- `confit.shell.SHELL`); the plan fold materializes init entries through
-- minijinja with the shell facts when it builds per-shell rc documents.
-- Entries without template syntax pass through byte-identical, including
-- hand-written inits naming one shell.
-- The shared mise path stays a literal so the plan folds every package
-- into one TOML document exactly as before.
--
-- Only existing primitives compose this module: confit.config,
-- confit.document plus its rc namespace, confit.patch, confit.shell, and
-- confit.path stays available to callers. Failures raise through
-- confit.plugin.helpers.error so they attribute this plugin file.

local MISE_PATH = "~/.config/mise/config.toml"

-- Splits caller opts into patch section plus entry opts.
--
-- The section names the rc list and defaults to `config`. The entry opts
-- hold the in_path guard plus any caller `when`. The section key never
-- reaches the entry constructor.
local function section_and_opts(guard, opts)
	local section = "config"
	local merged = { when = guard }
	if opts ~= nil then
		if type(opts) ~= "table" then
			confit.plugin.helpers.error("mise: rc opts must be a table")
		end
		for key, value in pairs(opts) do
			if key == "section" then
				section = value
			else
				merged[key] = value
			end
		end
	end
	if type(section) ~= "string" then
		confit.plugin.helpers.error("mise: field 'section' must be a string")
	end
	return section, merged
end

-- Builds the rc collector for one callback run.
--
-- Each method stashes one section plus entry pair. The caller drains
-- the stash into a single patch.rc after the callback returns, so one
-- callback run means one patch holding every add in call order.
-- Each entry carries the in_path guard for the binary. The guard
-- always applies.
local function collector(binary)
	local guard = confit.shell.in_path(binary)
	local pending = {}
	local function stash(section, entry)
		pending[#pending + 1] = { section, entry }
	end
	local rc = {}
	function rc:alias(name, value, opts)
		local section, entry_opts = section_and_opts(guard, opts)
		stash(section, confit.document.rc.alias(name, value, entry_opts))
	end
	function rc:env(name, value, opts)
		local section, entry_opts = section_and_opts(guard, opts)
		stash(section, confit.document.rc.env(name, value, entry_opts))
	end
	function rc:prepend(first, second, third)
		local var, dir, opts
		if third ~= nil or type(second) == "string" then
			var, dir, opts = first, second, third
		else
			var, dir, opts = nil, first, second
		end
		local section, entry_opts = section_and_opts(guard, opts)
		if var == nil then
			stash(section, confit.document.rc.prepend(dir, entry_opts))
		else
			stash(section, confit.document.rc.prepend(var, dir, entry_opts))
		end
	end
	function rc:eval(argv, opts)
		local section, entry_opts = section_and_opts(guard, opts)
		stash(section, confit.document.rc.eval(argv, entry_opts))
	end
	function rc:cmd(argv, opts)
		local section, entry_opts = section_and_opts(guard, opts)
		stash(section, confit.document.rc.cmd(argv, entry_opts))
	end
	function rc:source(path, opts)
		local section, entry_opts = section_and_opts(guard, opts)
		stash(section, confit.document.rc.source(path, entry_opts))
	end
	return rc, pending
end

-- Drains one stash into a single patch on the config.
--
-- Empty stashes add nothing, so callbacks holding no rc calls stay quiet.
local function flush(config, pending)
	if #pending == 0 then
		return
	end
	config:add_patch(confit.patch.rc(function(data)
		for _, item in ipairs(pending) do
			data:add(item[1], item[2])
		end
	end))
end

-- Shared mise config holding the single base document.
--
-- One path holds one document, so every package patches the shared base
-- instead of declaring it. The module chunk runs once per evaluation, so
-- the base declares exactly once no matter how many packages follow.
local shared = nil
local function shared_config()
	if shared == nil then
		shared = confit.config("mise")
		shared:add_document(confit.document.structured("toml", {
			path = MISE_PATH,
			data = { tools = {} },
		}))
	end
	return shared
end

-- Declares one mise package plus its callback rc entries.
--
-- Generates the config under the package name, folds the package into the
-- shared mise TOML at latest through a patch, then runs the optional
-- callback with the collector. Activation stays separate through
-- `activate()`.
local function package(name, callback)
	if type(name) ~= "string" or name == "" then
		confit.plugin.helpers.error("mise: field 'name' must be a non-empty string")
	end
	if callback ~= nil and type(callback) ~= "function" then
		confit.plugin.helpers.error("mise: field 'callback' must be a function or nil")
	end
	shared_config()
	local config = confit.config(name)
	config:add_patch(confit.patch.structured("toml", MISE_PATH, function(data)
		data:set("tools." .. name, "latest")
	end))
	if callback ~= nil then
		local rc, pending = collector(name)
		callback(rc)
		flush(config, pending)
	end
	return config
end

-- Returns the patch adding mise activation plus its binary path.
--
-- One patch holds both adds in order, so the PATH line always
-- renders before the activation eval. The eval carries the
-- `{{shell}}` template slot and stays unconditional. Callers
-- attach it with `config:add_patch`.
local function activate()
	return confit.patch.rc(function(data)
		data:add("profile", confit.document.rc.prepend(confit.path.home(".local/bin")))
		data:add("profile", confit.document.rc.eval({ "mise", "activate", confit.shell.SHELL }))
	end)
end

return { package = package, activate = activate }
