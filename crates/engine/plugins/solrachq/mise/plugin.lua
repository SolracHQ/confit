-- solrachq.mise default plugin.
--
-- Installer dialect over config plus document primitives:
-- `package(name, callback)` generates the config under the package name,
-- declares the mise install as a TOML document for the shared mise config
-- path, then runs the callback with an rc collector. Every callback entry
-- carries `when = confit.shell.in_path(binary)`, so shell lines stay inert
-- until the binary lands on PATH. `activate()` returns the eval entry for
-- mise activation with the `{{shell}}` slot. Callers attach it with
-- `config:add_document`.
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
-- confit.document plus its rc namespace, confit.shell, and confit.path
-- stays available to callers. Failures raise through
-- confit.plugin.helpers.error so they attribute this plugin file.

local MISE_PATH = "~/.config/mise/config.toml"

-- Builds the rc collector for one callback run.
--
-- Each method wraps the matching confit.document.rc constructor with the
-- in_path guard for the binary, then submits to the config. The guard
-- always applies; extra opts pass through untouched.
local function collector(config, binary)
  local guard = confit.shell.in_path(binary)
  local function opts_with_guard(opts)
    local merged = { when = guard }
    if opts ~= nil then
      if type(opts) ~= "table" then
        confit.plugin.helpers.error("mise: rc opts must be a table")
      end
      for key, value in pairs(opts) do
        merged[key] = value
      end
    end
    return merged
  end
  local rc = {}
  function rc:alias(name, value, opts)
    config:add_document(confit.document.rc.alias(name, value, opts_with_guard(opts)))
  end
  function rc:env(name, value, opts)
    config:add_document(confit.document.rc.env(name, value, opts_with_guard(opts)))
  end
  function rc:profile(name, value, opts)
    config:add_document(confit.document.rc.profile(name, value, opts_with_guard(opts)))
  end
  function rc:profile_path(dir, opts)
    config:add_document(confit.document.rc.profile_path(dir, opts_with_guard(opts)))
  end
  function rc:eval(argv, opts)
    config:add_document(confit.document.rc.eval(argv, opts_with_guard(opts)))
  end
  function rc:cmd(argv, opts)
    config:add_document(confit.document.rc.cmd(argv, opts_with_guard(opts)))
  end
  function rc:source(path, opts)
    config:add_document(confit.document.rc.source(path, opts_with_guard(opts)))
  end
  return rc
end

-- Declares one mise package plus its callback rc entries.
--
-- Generates the config under the package name, folds the package into the
-- shared mise TOML at latest, then runs the optional callback with the
-- collector. Activation stays separate through `activate()`.
local function package(name, callback)
  if type(name) ~= "string" or name == "" then
    confit.plugin.helpers.error("mise: field 'name' must be a non-empty string")
  end
  if callback ~= nil and type(callback) ~= "function" then
    confit.plugin.helpers.error("mise: field 'callback' must be a function or nil")
  end
  local config = confit.config(name)
  config:add_document(confit.document.structured("toml", {
    path = MISE_PATH,
    data = { tools = { [name] = "latest" } },
  }))
  if callback ~= nil then
    callback(collector(config, name))
  end
  return config
end

-- Returns the eval entry for mise activation.
--
-- Carries the `{{shell}}` template slot. The entry stays unconditional.
-- Callers attach it with `config:add_document`.
local function activate()
  return confit.document.rc.eval({ "mise", "activate", confit.shell.SHELL })
end

return { package = package, activate = activate }
