-- solrachq.mise default plugin.
--
-- Installer dialect over config plus patch primitives:
-- `package(opts)` takes a table holding the package name, the pinned
-- version defaulting to `latest`, plus the optional rc builder. It
-- generates the config under the package name, folds the version into
-- the shared mise TOML, declares the shared `mise install` hook, then
-- injects a require on the installer config, and finally runs the
-- builder with an rc collector. Every collected entry carries
-- `when = confit.runtime.in_path(binary)`, so shell lines stay inert
-- until the binary lands on PATH. `init(version?)` returns the
-- installer config holding the shared base document plus the mise
-- binary document, composed from `fetch_file` plus `compressed` plus
-- `opaque` exactly like the upstream installer: curl the release
-- tarball, unpack `mise/bin/mise`, place it under `~/.local/bin`
-- with mode `755`. It also carries the activation patch, PATH
-- prepend plus eval entry with the `{{shell}}` slot. An explicit
-- version wins; an omitted one resolves the latest release
-- from the releases feed.
--
-- Per-shell activation choice: the plugin runs during profile evaluation,
-- before the profile declares its shells, so it cannot name a shell here.
-- The activation argv carries the `{{shell}}` template slot (see
-- `confit.runtime.SHELL`); the plan fold materializes init entries through
-- minijinja with the shell facts when it builds per-shell rc documents.
-- Entries without template syntax pass through byte-identical, including
-- hand-written inits naming one shell.
-- The shared mise path stays a literal so the plan folds every package
-- into one TOML document exactly as before.
--
-- Only existing primitives compose this module: confit.config,
-- confit.document plus its rc namespace, confit.patch, confit.runtime,
-- confit.hook, plus confit.resources plus confit.path for the installer.
-- Failures raise through confit.plugin.helpers.error so they attribute
-- this plugin file.

local MISE_PATH = "~/.config/mise/config.toml"

-- Installer config name holding the shared base plus the mise binary.
-- Packages require this target, so a profile without `init()` fails
-- the plan naming both configs plus the hint.
local INSTALL_CONFIG = "plugin:solrachq/mise:install"

-- Hint rendered beside the missing installer config name.
local REQUIRE_HINT = "Add mise.init() to the profile configs."

-- Releases feed backing latest-version resolution for `init()`.
-- Latest release reads first, so the first tag wins.
local MISE_RELEASES_URL = "https://api.github.com/repos/jdx/mise/releases"

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
	local guard = confit.runtime.in_path(binary)
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

-- Resolves the mise release backing the installer tarball.
--
-- An explicit version wins as is. An omitted one reads the latest
-- release tag from the releases feed and strips the `v` to reach
-- the version number. Anything else fails as a plan error.
local function resolve_version(version)
	if version ~= nil then
		return version
	end
	local body = confit.resources.fetch_text(MISE_RELEASES_URL)
	local tag = body:match('"tag_name"%s*:%s*"([^"]+)"')
	if tag == nil then
		confit.plugin.helpers.error("mise: cannot resolve the latest release from the releases feed")
	end
	local stripped = tag:gsub("^v", "")
	if stripped:match("^%d+%.%d+") == nil then
		confit.plugin.helpers.error("mise: tag '" .. tag .. "' holds no version number")
	end
	return stripped
end

-- Builds the mise binary document from the release tarball.
--
-- The tarball holds `mise/bin/mise`; the pick places it under the home
-- binary dir with mode `755`. A tarball holding the member any other
-- number of times fails as a plan error.
local function installer_binary(version)
	local url = "https://github.com/jdx/mise/releases/download/v"
		.. version
		.. "/mise-v"
		.. version
		.. "-linux-x64.tar.gz"
	local archive = confit.resources.fetch_file(url)
	local bin_dir = confit.path.home(".local/bin")
	local picked = confit.document.compressed(archive, function(path, _, content)
		if path == "mise/bin/mise" then
			return confit.document.opaque(bin_dir .. "/mise", content, { mode = "755" })
		end
	end)
	if #picked ~= 1 then
		confit.plugin.helpers.error("mise: installer tarball holds mise/bin/mise exactly once")
	end
	return picked[1]
end

-- Shared installer config holding the base document plus the binary.
--
-- The module chunk runs once per evaluation, so the installer builds
-- exactly once no matter how many packages follow. Repeated calls
-- return the first installer.
local installer = nil

-- Returns the installer config holding the mise base plus the binary.
--
-- The config carries the installer name, so package requires resolve
-- against it. It holds documents alone, no hooks. Callers list it in
-- the profile configs beside their packages.
local function init(version)
	if version ~= nil and (type(version) ~= "string" or version == "") then
		confit.plugin.helpers.error("mise: field 'version' must be a non-empty string or nil")
	end
	if installer == nil then
		local resolved = resolve_version(version)
		installer = confit.config(INSTALL_CONFIG)
		installer:add_document(confit.document.structured("toml", {
			path = MISE_PATH,
			data = { tools = {} },
		}))
		installer:add_document(installer_binary(resolved))
		installer:add_patch(confit.patch.rc(function(data)
			data:add("profile", confit.document.rc.prepend(confit.path.home(".local/bin")))
			data:add("profile", confit.document.rc.eval({ "mise", "activate", confit.runtime.SHELL }))
		end))
	end
	return installer
end

-- Declares one mise package plus its builder rc entries.
--
-- The table holds `name` plus `version` defaulting to `latest` plus the
-- optional `rc_builder`. The package folds its version into the shared
-- mise TOML, declares the shared install hook, then requires the
-- installer config, then runs the builder with the collector.
local function package(opts)
	if type(opts) ~= "table" then
		confit.plugin.helpers.error("mise: package opts must be a table")
	end
	local name = opts.name
	local version = opts.version
	local bin = opts.bin
	local aliases = opts.aliases
	local options = opts.options
	local rc_builder = opts.rc_builder
	if type(name) ~= "string" or name == "" then
		confit.plugin.helpers.error("mise: field 'name' must be a non-empty string")
	end
	if version == nil then
		version = "latest"
	elseif type(version) ~= "string" or version == "" then
		confit.plugin.helpers.error("mise: field 'version' must be a non-empty string")
	end
	if bin == nil then
		bin = name
	elseif type(bin) ~= "string" or bin == "" then
		confit.plugin.helpers.error("mise: field 'bin' must be a non-empty string")
	end
	if rc_builder ~= nil and type(rc_builder) ~= "function" then
		confit.plugin.helpers.error("mise: field 'rc_builder' must be a function or nil")
	end
	if aliases ~= nil and type(aliases) ~= "table" then
		confit.plugin.helpers.error("mise: field 'aliases' must be a table")
	end
	if options ~= nil then
		if type(options) ~= "table" then
			confit.plugin.helpers.error("mise: field 'options' must be a table")
		end
		for key, value in pairs(options) do
			if type(key) ~= "string" or key == "" then
				confit.plugin.helpers.error("mise: field 'options' keys must be non-empty strings")
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
						confit.plugin.helpers.error(field)
					end
					if item_key > max then
						max = item_key
					end
					count = count + 1
					local item_type = type(item)
					if item_type ~= "string" and item_type ~= "number" and item_type ~= "boolean" then
						confit.plugin.helpers.error(field)
					end
				end
				if max ~= count then
					confit.plugin.helpers.error(field)
				end
			else
				confit.plugin.helpers.error(field)
			end
		end
	end
	for key, _ in pairs(opts) do
		if key ~= "name" and key ~= "version" and key ~= "bin" and key ~= "aliases" and key ~= "options" and key ~= "rc_builder" then
			confit.plugin.helpers.error("mise: field 'opts' unknown field '" .. tostring(key) .. "'")
		end
	end
	local config = confit.config(name)
	config:add_patch(confit.patch.structured("toml", MISE_PATH, function(data)
		if options == nil then
			data:set("tools." .. name, version)
		else
			local entry = {}
			for key, value in pairs(options) do
				entry[key] = value
			end
			entry.version = version
			data:set("tools." .. name, entry)
		end
	end))
	config:require(INSTALL_CONFIG, REQUIRE_HINT)
	config:add_hook(confit.hook.run({ "mise", "install" }, {
		path = { confit.path.home(".local/bin") },
		requires = confit.runtime.in_path("mise"),
		when = confit.runtime.changed("~/.config/mise/config.toml"),
		checks = { confit.runtime.exists(confit.path.data("mise/shims/" .. bin)) },
	}))
	if rc_builder ~= nil then
		local rc, pending = collector(bin)
		rc_builder(rc)
		flush(config, pending)
	end
	if aliases ~= nil then
		local names = {}
		for alias_name, expansion in pairs(aliases) do
			if type(alias_name) ~= "string" or alias_name == "" then
				confit.plugin.helpers.error("mise: field 'aliases' keys must be non-empty strings")
			end
			if type(expansion) ~= "string" or expansion == "" then
				confit.plugin.helpers.error("mise: field 'aliases' values must be non-empty strings")
			end
			names[#names + 1] = alias_name
		end
		table.sort(names)
		local rc, pending = collector(bin)
		for _, alias_name in ipairs(names) do
			rc:alias(alias_name, aliases[alias_name])
		end
		flush(config, pending)
	end
	return config
end

return { package = package, init = init }
