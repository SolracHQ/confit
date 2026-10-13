---@module "plugins._.mise.release"

local fetch = require("confit.fetch")
local is = require("plugins._.is")

--- Releases feed backing latest-version resolution.
local MISE_RELEASES_URL = "https://api.github.com/repos/jdx/mise/releases"

--- Resolves the mise release backing the installer tarball.
---@param version string? Pinned version or nil for the latest tag.
---@return string Version number without the v prefix.
local function resolve_version(version)
	if version then
		return version
	end
	local handle = is.must(
		"mise: cannot resolve the latest release from the releases feed: ",
		fetch(MISE_RELEASES_URL)
	)
	local body = is.must(
		"mise: cannot resolve the latest release from the releases feed: ",
		handle:text()
	)
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
---@param version string Release version of the tarball.
---@return ResourceHandle Resource handle holding the mise binary.
local function installer_binary(version)
	local url = "https://github.com/jdx/mise/releases/download/v"
		.. version
		.. "/mise-v"
		.. version
		.. "-linux-x64.tar.gz"
	local archive_handle =
		is.must("mise: cannot fetch the installer tarball: ", fetch(url))
	local archive = is.must("mise: cannot open the installer tarball: ", archive_handle:archive())
	return is.must("mise: the installer tarball holds no mise/bin/mise: ", archive:member("mise/bin/mise"))
end

return { resolve = resolve_version, binary = installer_binary }
