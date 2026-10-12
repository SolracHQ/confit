---@module "plugins._.nerd_fonts"

--- Installs nerd fonts through per-font tree documents and hooks.
local nerd_fonts = {}

local fetch = require("confit.fetch")
local validate = require("confit.validate")

--- Releases feed backing latest-version resolution; the first tag wins.
local FONTS_RELEASES_URL = "https://api.github.com/repos/ryanoasis/nerd-fonts/releases"

--- Checks one built config table, failing on unknown keys.
---@param config Config Config table under checking.
local function check_config(config)
	local _, reason = validate.config(config)
	if reason then
		error("nerd_fonts: invalid config: " .. tostring(reason))
	end
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
	local handle, fetch_reason = fetch(FONTS_RELEASES_URL)
	if not handle then
		error("nerd_fonts: cannot resolve the latest release from the releases feed: " .. tostring(fetch_reason))
	end
	local body, text_reason = handle:text()
	if not body then
		error("nerd_fonts: cannot resolve the latest release from the releases feed: " .. tostring(text_reason))
	end
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
	local names, names_reason = archive:members()
	if not names then
		error("nerd_fonts: cannot list the font archive: " .. tostring(names_reason))
	end
	local members = {}
	for _, name in ipairs(names) do
		if name:match("%.ttf$") then
			local member, member_reason = archive:member(name)
			if not member then
				error("nerd_fonts: the font archive holds no '" .. name .. "': " .. tostring(member_reason))
			end
			members[name] = member
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
	if type(name) ~= "string" or name == "" then
		error("nerd_fonts: field 'name' must be a non-empty string")
	end
	if version and (type(version) ~= "string" or version == "") then
		error("nerd_fonts: field 'version' must be a non-empty string or nil")
	end
	local resolved = resolve_version(version)
	local url = "https://github.com/ryanoasis/nerd-fonts/releases/download/v"
		.. resolved
		.. "/"
		.. name
		.. ".zip"
	local handle, fetch_reason = fetch(url)
	if not handle then
		error("nerd_fonts: cannot fetch the font zip: " .. tostring(fetch_reason))
	end
	local archive, archive_reason = handle:archive()
	if not archive then
		error("nerd_fonts: cannot open the font zip: " .. tostring(archive_reason))
	end
	local dest = font_route(name)
	local config = {
		name = name,
		documents = { [dest] = { members = ttf_members(archive) } },
		hooks = { refresh_hook(dest) },
	}
	check_config(config)
	return config
end

return nerd_fonts
