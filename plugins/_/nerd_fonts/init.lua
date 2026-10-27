---@module "plugins._.nerd_fonts"

--- Installs nerd fonts through per-font tree documents and hooks.
local nerd_fonts = {}

local fetch = require("confit.fetch")
local is = require("plugins._.is")
local validate = require("confit.validate")

--- Releases feed backing latest-version resolution.
local FONTS_RELEASES_URL = "https://api.github.com/repos/ryanoasis/nerd-fonts/releases"

--- Checks one built config table, failing on unknown keys.
---@param config Config Config table under checking.
local function check_config(config)
	is.must("nerd_fonts: invalid config: ", validate.config(config))
end

--- Builds the destination route holding one font.
---@param name string Font name landing under the data fonts folder.
---@return Route Route table with the data base.
local function font_route(name)
	return { base = "data", relative = "fonts/" .. name }
end

--- Resolves the release backing one font zip.
---@param version string? Pinned version or nil for the latest tag.
---@return string Version number without the v prefix.
local function resolve_version(version)
	if version then
		return version
	end
	local handle = is.must(
		"nerd_fonts: cannot resolve the latest release from the releases feed: ",
		fetch(FONTS_RELEASES_URL)
	)
	local body = is.must(
		"nerd_fonts: cannot resolve the latest release from the releases feed: ",
		handle:text()
	)
	local tag = body:match('"tag_name"%s*:%s*"([^"]+)"')
	if not tag then
		error("nerd_fonts: cannot resolve the latest release from the releases feed")
	end
	local stripped = tag:gsub("^v", "")
	if stripped:match("^%d+%.%d+") == nil then
		error("nerd_fonts: tag '" .. tag .. "' holds no version number")
	end
	return stripped
end

--- Collects the ttf members of one archive into a flat handle map.
---@param archive ArchiveHandle Opened font zip.
---@return {[string]: ResourceHandle} Member handles keyed by archive path.
local function ttf_members(archive)
	local names =
		is.must("nerd_fonts: cannot list the font archive: ", archive:members())
	local members = {}
	for _, name in ipairs(names) do
		if name:match("%.ttf$") then
			members[name] =
				is.must("nerd_fonts: the font archive holds no '" .. name .. "': ", archive:member(name))
		end
	end
	if not next(members) then
		error("nerd_fonts: the font archive holds no ttf members")
	end
	return members
end

--- Builds the cache refresh hook for one font folder.
---@param dest Route Font folder route refreshed alone.
---@return Hook Hook running fc-cache with conditions.
local function refresh_hook(dest)
	return {
		argv = { "fc-cache", "-f", dest },
		requires = { in_path = { name = "fc-cache" } },
		when = { changed = { route = dest } },
	}
end

--- Declares one nerd font plus its refresh hook.
---@param name string Font name picking the release zip.
---@param version string? Pinned release defaulting to latest.
---@return Config Config table for the profile configs array.
function nerd_fonts.font(name, version)
	is.assert(is.nonempty, name, "nerd_fonts: field 'name' must be a non-empty string")
	if version then
		is.assert(is.nonempty, version, "nerd_fonts: field 'version' must be a non-empty string or nil")
	end
	local resolved = resolve_version(version)
	local url = "https://github.com/ryanoasis/nerd-fonts/releases/download/v"
		.. resolved
		.. "/"
		.. name
		.. ".zip"
	local handle = is.must("nerd_fonts: cannot fetch the font zip: ", fetch(url))
	local archive = is.must("nerd_fonts: cannot open the font zip: ", handle:archive())
	local dest = font_route(name)
	local config = {
		name = id(name),
		documents = { { path = dest, members = ttf_members(archive) } },
		hooks = { refresh_hook(dest) },
	}
	check_config(config)
	return config
end

return nerd_fonts
