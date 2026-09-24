-- solrachq.nerd_fonts default plugin.
--
-- Installer dialect over config plus hook primitives:
-- `font(name, version?)` takes the nerd font name plus the pinned
-- release defaulting to `latest`. It fetches the release zip,
-- builds one tree document holding the fonts under the managed
-- folder `fonts/{name}`, then declares the refresh hook scoped
-- to that folder. Each font carries its own hook argv, so every
-- font refresh runs on its own with no shared installer config.
-- The hook carries no checks: a cache rebuild holds no stable
-- disk proof, so it fires every apply while `fc-cache` resolves.
--
-- Only existing primitives compose this module: confit.config,
-- confit.document plus confit.hook, confit.runtime plus
-- confit.fetch plus confit.path. Failures raise through
-- confit.plugin.helpers.error so they attribute this plugin file.

-- Releases feed backing latest-version resolution for `font()`.
-- Latest release reads first, so the first tag wins.
local FONTS_RELEASES_URL = "https://api.github.com/repos/ryanoasis/nerd-fonts/releases"

-- Managed destination holding one font, nested under the data fonts
-- folder so each font lands apart from the rest.
local function managed_dest(name)
	return confit.path.data("fonts", name)
end

-- Resolves the nerd fonts release backing one font zip.
--
-- An explicit version wins as is. An omitted one reads the
-- latest release tag from the releases feed and strips the `v`
-- to reach the version number. Anything else fails as a plan error.
local function resolve_version(version)
	if version ~= nil then
		return version
	end
	local body = confit.fetch(FONTS_RELEASES_URL):text()
	local tag = body:match('"tag_name"%s*:%s*"([^"]+)"')
	if tag == nil then
		confit.plugin.helpers.error("nerd_fonts: cannot resolve the latest release from the releases feed")
	end
	local stripped = tag:gsub("^v", "")
	if stripped:match("^%d+%.%d+") == nil then
		confit.plugin.helpers.error("nerd_fonts: tag '" .. tag .. "' holds no version number")
	end
	return stripped
end

-- Declares one nerd font plus its refresh hook.
--
-- The name picks the release zip, the version pins the
-- release defaulting to `latest`. The font lands flattened
-- under the managed `fonts/{name}` folder, then the hook
-- refreshes that folder alone.
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
	local archive = confit.fetch(url)
	local dest = managed_dest(name)
	local tree = archive:tree(dest, function(member)
		if not member:name():match("%.ttf$") then
			return nil
		end
		return { path = member:name() }
	end)
	local config = confit.config(name)
	config:add_document(tree)
	config:add_hook(confit.hook.run({ "fc-cache", "-f", dest }, {
		requires = confit.runtime.in_path("fc-cache"),
		when = confit.runtime.changed(dest),
	}))
	return config
end

return { font = font }
