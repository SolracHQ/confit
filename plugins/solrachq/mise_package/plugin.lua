-- solrachq.mise_package default plugin.
--
-- Single-call installer dialect over config plus artifact primitives:
-- `mise_package(name, callback)` generates the config internally under the
-- package name, declares the mise install as a TOML artifact for the shared
-- mise config path plus an unconditional activation init, then runs the
-- callback with an rc collector. Every callback entry carries
-- `when = confit.shell.in_path(binary)`, so shell lines stay inert until
-- the binary lands on PATH.
--
-- Per-shell activation choice: the plugin runs during profile evaluation,
-- before the profile declares its shells, so it cannot name a shell here.
-- The activation argv carries the `{{shell}}` template slot (see
-- `confit.shell.SHELL`); the plan fold materializes init entries through
-- minijinja with the shell facts when it builds per-shell rc artifacts.
-- Entries without template syntax pass through byte-identical, including
-- hand-written inits naming one shell.
-- The shared mise path stays a literal so the plan folds every package
-- into one TOML artifact exactly as before.
--
-- Only existing primitives compose this module: confit.config,
-- confit.artifact plus its rc namespace, confit.shell, and confit.path
-- stays available to callers. Failures raise through
-- confit.plugin.helpers.error so they attribute this plugin file.

local MISE_PATH = "~/.config/mise/config.toml"

-- Builds the rc collector for one callback run.
--
-- Each method wraps the matching confit.artifact.rc constructor with the
-- in_path guard for the binary, then submits to the config. An optional
-- opts table carries `priority` through; the guard always applies.
local function collector(config, binary)
  local guard = confit.shell.in_path(binary)
  local function opts_with_guard(opts)
    local merged = { when = guard }
    if opts ~= nil then
      if type(opts) ~= "table" then
        confit.plugin.helpers.error("mise_package: rc opts must be a table")
      end
      if opts.priority ~= nil then
        merged.priority = opts.priority
      end
    end
    return merged
  end
  local rc = {}
  function rc:alias(name, value, opts)
    config:add_artifact(confit.artifact.rc.alias(name, value, opts_with_guard(opts)))
  end
  function rc:env(name, value, opts)
    config:add_artifact(confit.artifact.rc.env(name, value, opts_with_guard(opts)))
  end
  function rc:profile(name, value, opts)
    config:add_artifact(confit.artifact.rc.profile(name, value, opts_with_guard(opts)))
  end
  function rc:profile_path(dir, opts)
    config:add_artifact(confit.artifact.rc.profile_path(dir, opts_with_guard(opts)))
  end
  function rc:init(spec, opts)
    config:add_artifact(confit.artifact.rc.init(spec, opts_with_guard(opts)))
  end
  return rc
end

-- Declares one mise package plus its callback rc entries.
--
-- Generates the config under the package name, folds the package into the
-- shared mise TOML at latest, adds the marker activation init
-- unconditionally, then runs the optional callback with the collector.
local function mise_package(name, callback)
  if type(name) ~= "string" or name == "" then
    confit.plugin.helpers.error("mise_package: field 'name' must be a non-empty string")
  end
  if callback ~= nil and type(callback) ~= "function" then
    confit.plugin.helpers.error("mise_package: field 'callback' must be a function or nil")
  end
  local config = confit.config(name)
  config:add_artifact(confit.artifact.toml(MISE_PATH, { tools = { [name] = "latest" } }))
  config:add_artifact(
    confit.artifact.rc.init({ eval = { "mise", "activate", confit.shell.SHELL } })
  )
  if callback ~= nil then
    callback(collector(config, name))
  end
  return config
end

return mise_package
