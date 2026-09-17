-- solrachq.nerd_fonts default plugin.
--
-- Installer dialect over config plus hook primitives:
-- `font(name, version?)` takes the nerd font name plus the pinned
-- release defaulting to `latest`. It fetches the release zip,
-- builds one tree document holding the fonts under the managed
-- folder, declares the shared `fc-cache` hook, then injects a
-- require on the installer config. `init()` returns the
-- installer config holding the shared refresh hook, so the
-- hook anchor stays declared even beside a single font.
-- Merged hooks run the refresh once per apply: a cache rebuild
-- holds no stable disk proof, so the hook carries no checks
-- and fires every apply while `fc-cache` resolves.
--
-- Only existing primitives compose this module: confit.config,
-- confit.document plus confit.hook, confit.runtime plus
-- confit.resources plus confit.path. Failures raise through
-- confit.plugin.helpers.error so they attribute this plugin file.

-- Installer config name holding the shared refresh hook.
-- Fonts require this target, so a profile without `init()`
-- fails the plan naming both configs plus the hint.
local INSTALL_CONFIG = "plugin:solrachq/nerd_fonts:install"

-- Hint rendered beside the missing installer config name.
local REQUIRE_HINT = "Add nerd_fonts.init() to the profile configs."

-- Tags feed backing latest-version resolution for `font()`.
local FONTS_TAGS_URL = "https://api.github.com/repos/ryanoasis/nerd-fonts/tags"

-- Refresh argv shared by every font hook, merged into one run.
local REFRESH_ARGV = { "fc-cache", "-f" }

-- Managed folder holding confit fonts, deeper than the bare
-- data fonts folder since fc-cache walks the whole folder.
local function managed_dir()
	return confit.path.data("fonts", "confit")
end

-- Resolves the nerd fonts release backing one font zip.
--
-- An explicit version wins as is. An omitted one reads the
-- latest tag from the tags feed and strips the `v` to reach
-- the version number. Anything else fails as a plan error.
local function resolve_version(version)
	if version ~= nil then
		return version
	end
	local body = confit.resources.fetch_text(FONTS_TAGS_URL)
	local tag = body:match('"name"%s*:%s*"([^"]+)"')
	if tag == nil then
		confit.plugin.helpers.error("nerd_fonts: cannot resolve the latest release from the tags feed")
	end
	local stripped = tag:gsub("^v", "")
	if stripped:match("^%d+%.%d+") == nil then
		confit.plugin.helpers.error("nerd_fonts: tag '" .. tag .. "' holds no version number")
	end
	return stripped
end

-- Builds the shared refresh hook.
--
-- The hook carries no checks: a cache rebuild holds no stable
-- disk proof, so it fires every apply while `fc-cache` resolves.
local function refresh_hook()
	return confit.hook.run(REFRESH_ARGV, { when = confit.runtime.in_path("fc-cache") })
end

-- Shared installer config holding the refresh hook.
--
-- The module chunk runs once per evaluation, so the installer
-- builds exactly once no matter how many fonts follow.
-- Repeated calls return the first installer.
local installer = nil

-- Returns the installer config holding the refresh hook.
--
-- The config carries the installer name, so font requires
-- resolve against it. It holds the hook alone, no documents.
-- Callers list it in the profile configs beside their fonts.
local function init()
	if installer == nil then
		installer = confit.config(INSTALL_CONFIG)
		installer:add_hook(refresh_hook())
	end
	return installer
end

-- Declares one nerd font plus its refresh hook.
--
-- The name picks the release zip, the version pins the
-- release defaulting to `latest`. The font lands flattened
-- under the managed folder, then requires the installer config.
local function font(name, version)
	if type(name) ~= "string" or name == "" then
		confit.plugin.helpers.error("nerd_fonts: field 'name' must be a non-empty string")
	end
	if version ~= nil and (type(version) ~= "string" or version == "") then
		confit.plugin.helpers.error("nerd_fonts: field 'version' must be a non-empty string or nil")
	end
	local resolved = resolve_version(version)
	local url = "https://github.com/ryanoasis/nerd-fonts/releases/download/v"
		.. resolved
		.. "/"
		.. name
		.. ".zip"
	local archive = confit.resources.fetch_file(url)
	local dest = managed_dir()
	local tree = confit.document.tree(archive, dest, function(path, _, _)
		if not path:match("%.ttf$") then
			return nil
		end
		return path:match("([^/]+)$")
	end)
	local config = confit.config(name)
	config:add_document(tree)
	config:require(INSTALL_CONFIG, REQUIRE_HINT)
	config:add_hook(refresh_hook())
	return config
end

return { font = font, init = init }
