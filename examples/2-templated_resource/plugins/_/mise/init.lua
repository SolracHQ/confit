---@module "plugins._.mise"

--- Installs mise tools through plain configs and patches.
local mise = {}

local validate = require("confit.validate")
local speclib = require("plugins._.mise.spec")
local rc_lib = require("plugins._.mise.rc")
local release = require("plugins._.mise.release")

---@type Route
local MISE_TOML = { base = "config", relative = "mise/config.toml" }
---@type Route
local MISE_BIN = { base = "home", relative = ".local/bin/mise" }
---@type Route
local MISE_BINDIR = { base = "home", relative = ".local/bin" }

--- Installer config name; packages without it fail on the missing target.
local INSTALLER_NAME = "mise"

--- Checks one built config table, failing on unknown keys.
---@param config Config Config table under checking.
local function check_config(config)
	local _, reason = validate.config(config)
	if reason then
		error("mise: invalid config: " .. tostring(reason))
	end
end

--- Shared installer config across one evaluation.
local installer = nil

--- Returns the installer config holding the shared base and the binary.
---@param version string? Pinned mise release resolving latest when absent.
---@return Config Installer config table for the profile configs array.
function mise.init(version)
	if version and (type(version) ~= "string" or version == "") then
		error("mise: field 'version' must be a non-empty string or nil")
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
				local entry = {}
				for key, value in pairs(options) do
					entry[key] = value
				end
				entry.version = version
				data.data.tools[name] = entry
			end
		end,
	}
end

--- Builds the install hook for one package binary.
---@param bin string Binary proving the install through its shim.
---@return Hook Hook running mise install with conditions.
local function install_hook(bin)
	return {
		argv = { "mise", "install" },
		path = { MISE_BINDIR },
		requires = { in_path = { name = "mise" } },
		when = { changed = { route = MISE_TOML } },
		checks = { { exists = { route = { base = "data", relative = "mise/shims/" .. bin } } } },
		timeout = "10m",
	}
end

--- Declares one mise package and its builder rc entries.
---@param spec MiseSpec Package table holding name, version, options, rc_builder.
---@return Config Config table for the profile configs array.
function mise.package(spec)
	local name, version, bin, aliases, options, rc_builder = speclib.check(spec)
	local config = {
		name = name,
		patches = { fold_patch(name, version, options) },
		hooks = { install_hook(bin) },
	}
	for _, patch in ipairs(rc_lib.rc_patches(bin, rc_builder, aliases)) do
		table.insert(config.patches, patch)
	end
	check_config(config)
	return config
end

return mise
