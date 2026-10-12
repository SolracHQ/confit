---@module "plugins._.mise.release"

local fetch = require("confit.fetch")

--- Releases feed backing latest-version resolution; the first tag wins.
local MISE_RELEASES_URL = "https://api.github.com/repos/jdx/mise/releases"

--- Resolves the mise release backing the installer tarball.
---@param version string? Pinned version or nil for the latest tag.
---@return string Version number without the v prefix.
local function resolve_version(version)
	if version then
		return version
	end
	local handle, reason = fetch(MISE_RELEASES_URL)
	if not handle then
		error("mise: cannot resolve the latest release from the releases feed: " .. tostring(reason))
	end
	local body, text_reason = handle:text()
	if not body then
		error("mise: cannot resolve the latest release from the releases feed: " .. tostring(text_reason))
	end
	local tag = body:match('"tag_name"%s*:%s*"([^"]+)"')
	if not tag then
		error("mise: cannot resolve the latest release from the releases feed")
	end
	local stripped = tag:gsub("^v", "")
	if stripped:match("^%d+%.%d+") == nil then
		error("mise: tag '" .. tag .. "' holds no version number")
	end
	return stripped
end

--- Builds the mise binary handle from the release tarball.
---@param version string Release version naming the tarball.
---@return ResourceHandle Resource handle holding the mise binary.
local function installer_binary(version)
	local url = "https://github.com/jdx/mise/releases/download/v"
		.. version
		.. "/mise-v"
		.. version
		.. "-linux-x64.tar.gz"
	local archive_handle, fetch_reason = fetch(url)
	if not archive_handle then
		error("mise: cannot fetch the installer tarball: " .. tostring(fetch_reason))
	end
	local archive, archive_reason = archive_handle:archive()
	if not archive then
		error("mise: cannot open the installer tarball: " .. tostring(archive_reason))
	end
	local member, member_reason = archive:member("mise/bin/mise")
	if not member then
		error("mise: the installer tarball holds no mise/bin/mise: " .. tostring(member_reason))
	end
	return member
end

return { resolve = resolve_version, binary = installer_binary }
