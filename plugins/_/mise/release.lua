---@module "plugins._.mise.release"

local fact = require("confit.fact")
local fetch = require("confit.fetch")
local is = require("plugins._.is")

--- Releases feed backing latest-version resolution.
local MISE_RELEASES_URL = "https://api.github.com/repos/jdx/mise/releases"

--- Installer asset suffix by host platform from fact("os") and fact("arch").
local PLATFORM_ASSET = {
	["linux/x86_64"] = "linux-x64",
	["linux/aarch64"] = "linux-arm64",
	["macos/x86_64"] = "macos-x64",
	["macos/aarch64"] = "macos-arm64",
}

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
	local platform = fact("os") .. "/" .. fact("arch")
	local asset = PLATFORM_ASSET[platform]
	if not asset then
		error("mise: unsupported platform '" .. platform .. "'")
	end
	local url = "https://github.com/jdx/mise/releases/download/v"
		.. version
		.. "/mise-v"
		.. version
		.. "-"
		.. asset
		.. ".tar.gz"
	local archive_handle =
		is.must("mise: cannot fetch the installer tarball: ", fetch(url))
	local archive = is.must("mise: cannot open the installer tarball: ", archive_handle:archive())
	return is.must("mise: the installer tarball holds no mise/bin/mise: ", archive:member("mise/bin/mise"))
end

return { resolve = resolve_version, binary = installer_binary }
