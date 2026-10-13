---@module "plugins._.mise"

--- Installs mise tools through plain configs and patches.
local mise = {}

local validate = require("confit.validate")
local is = require("plugins._.is")
local tab = require("plugins._.tab")
local speclib = require("plugins._.mise.spec")
local rc_lib = require("plugins._.mise.rc")
local release = require("plugins._.mise.release")

---@type Route
local MISE_TOML = { base = "config", relative = "mise/config.toml" }
---@type Route
local MISE_BIN = { base = "home", relative = ".local/bin/mise" }
---@type Route
local MISE_BINDIR = { base = "home", relative = ".local/bin" }

--- Installer config name.
local INSTALLER_NAME = "plugin:_/mise:install"

--- Installer hint.
local INSTALLER_HINT = "add mise.init() to the profile configs"

--- Shared install hook id.
local HOOK_ID = "plugin:_/mise:install"

--- Checks one built config table, failing on unknown keys.
---@param config Config Config table under checking.
local function check_config(config)
	is.must("mise: invalid config: ", validate.config(config))
end

--- Shared installer config across one evaluation.
local installer = nil

--- Builds the shared install hook carrying the id.
---@return Hook Hook running mise install with conditions.
local function install_hook()
	return {
		id = HOOK_ID,
		argv = { "mise", "install" },
		path = { MISE_BINDIR },
		requires = { in_path = { name = "mise" } },
		when = { changed = { route = MISE_TOML } },
		timeout = "10m",
	}
end

--- Returns the installer config holding the shared base and the binary.
---@param version string? Pinned mise release resolving latest when absent.
---@return Config Installer config table for the profile configs array.
function mise.init(version)
	if version then
		is.assert(is.nonempty, version, "mise: field 'version' must be a non-empty string or nil")
	end
	if not installer then
		local resolved = release.resolve(version)
		installer = {
			name = INSTALLER_NAME,
			documents = {
				[MISE_TOML] = { format = "toml", data = { tools = {} } },
				[MISE_BIN] = { handle = release.binary(resolved), mode = "755" },
			},
			patches = {
				{
					filter = rc_lib.is_shell,
					---@param route Route Shell route under reading.
					---@param data RcData Live shell sections under patching.
					apply = function(route, data)
						table.insert(data.profile, { path = { name = "PATH", dir = MISE_BINDIR } })
						table.insert(data.profile, { eval = { argv = { "mise", "activate", rc_lib.shell_of(route) } } })
					end,
				},
			},
			hooks = { install_hook() },
		}
		check_config(installer)
	end
	return installer
end

--- Builds the TOML fold patch for one package.
---@param name string Package and config name.
---@param version string Pinned version.
---@param options {[string]: any}? Backend tool options or nil.
---@return StructuredPatch Structured patch over the installer base.
local function fold_patch(name, version, options)
	return {
		target = MISE_TOML,
		format = "toml",
		---@param data Document_Structured Live TOML document under patching.
		apply = function(_, data)
			if not options then
				data.data.tools[name] = version
			else
				local entry = tab.clone(options)
				entry.version = version
				data.data.tools[name] = entry
			end
		end,
	}
end

--- Folds one shim check into the held check with AND flattening.
---@param held Condition? Held check condition or nil.
---@param next Condition Next shim check condition.
---@return Condition Joined check condition.
local function join_check(held, next)
	if held == nil then
		return next
	end
	if held.all ~= nil then
		local members = tab.clone(held.all)
		table.insert(members, next)
		return { all = members }
	end
	return { all = { held, next } }
end

--- Builds the hook patch appending one package shim check.
---@param bin string Binary proving the install through its shim.
---@return HookPatch Hook patch targeting the shared install hook.
local function shim_patch(bin)
	return {
		target = HOOK_ID,
		---@param hook Hook Live install hook under patching.
		apply = function(hook)
			hook.check =
				join_check(hook.check, { exists = { route = { base = "data", relative = "mise/shims/" .. bin } } })
		end,
	}
end

--- Declares one mise package and its rc entries.
---@param spec MiseSpec Package table holding name, version, options, rc.
---@return Config Config table for the profile configs array.
function mise.package(spec)
	local name, version, bin, options, rc = speclib.check(spec)
	local config = {
		name = name,
		requirements = { [INSTALLER_NAME] = INSTALLER_HINT },
		patches = { fold_patch(name, version, options), shim_patch(bin) },
	}
	for _, patch in ipairs(rc_lib.rc_patches(bin, rc)) do
		table.insert(config.patches, patch)
	end
	check_config(config)
	return config
end

return mise
