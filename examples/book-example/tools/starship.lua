local sugar = require("plugins._.sugar")
local is = require("plugins._.is")
local open = require("confit.open")
local fetch = require("confit.fetch")
local fact = require("confit.fact")

--- Release version pinned for the binary.
local STARSHIP_VERSION = "1.23.0"

--- Release asset and digest by host platform from fact("os") and fact("arch").
local STARSHIP_PLATFORM = {
	["linux/x86_64"] = {
		asset = "starship-x86_64-unknown-linux-gnu.tar.gz",
		sha256 = "cef41df04378c6f692913c5d9c1032d3b9a4369a1d2f3296c8300ed8838c2197",
	},
	["linux/aarch64"] = {
		asset = "starship-aarch64-unknown-linux-musl.tar.gz",
		sha256 = "d37040138c68b5f3334bbe16d4615f0a83829deebecc5d014e779c302e8270d8",
	},
	["macos/x86_64"] = {
		asset = "starship-x86_64-apple-darwin.tar.gz",
		sha256 = "8cac7a18fb1faf0aec50bd221dd71ecc8553850d99c212231851ae64ba11f089",
	},
	["macos/aarch64"] = {
		asset = "starship-aarch64-apple-darwin.tar.gz",
		sha256 = "042c8001275316836a3c43fdb88d0787395edfd0c10d209e4892ab5577b80d57",
	},
}

--- Archive member path of the binary.
local STARSHIP_MEMBER = "starship"

--- Base config resource.
local STARSHIP_RESOURCE = "resources/starship.toml"

--- Home dot-rc routes carry shell documents.
local SHELL_PATTERN = "^%.(%a+)rc$"

---@type Route
local STARSHIP_BIN = route("home", ".local/bin/starship")
---@type Route
local STARSHIP_TOML = route("config", "starship.toml")

--- Builds the starship config from the pinned release and the base TOML.
---@param user_config table User keys winning only where the base holds none.
---@return Config Starship config for the profile configs array.
return function(user_config)
	local platform = fact("os") .. "/" .. fact("arch")
	local release =
		is.must("starship: ", STARSHIP_PLATFORM[platform], "unsupported platform '" .. platform .. "'")
	local url = "https://github.com/starship/starship/releases/download/v"
		.. STARSHIP_VERSION
		.. "/"
		.. release.asset
	local archive_handle =
		is.must("starship: ", fetch(url, release.sha256), "cannot fetch the release tarball")
	local archive = is.must("starship: ", archive_handle:archive(), "cannot open the release tarball")
	local binary =
		is.must("starship: ", archive:member(STARSHIP_MEMBER), "the release tarball holds no " .. STARSHIP_MEMBER)
	local resource = is.must("starship: ", open(STARSHIP_RESOURCE), "cannot open " .. STARSHIP_RESOURCE)
	local base = is.must("starship: ", resource:load(), "cannot load " .. STARSHIP_RESOURCE)

	local config = {
		name = id("starship"),
		documents = {
			{ path = STARSHIP_BIN, handle = binary, mode = "755" },
			{ path = STARSHIP_TOML, format = "toml", data = base },
		},
		patches = {
			{
				filter = function(route)
					return route.base == "home" and route.relative:match(SHELL_PATTERN) ~= nil
				end,
				---@param document Document_Rc Live shell document under patching.
				apply = function(document)
					local shell = document.path.relative:match(SHELL_PATTERN)
					if not shell then
						error("starship: route '" .. document.path.relative .. "' holds no shell name")
					end
					table.insert(document.final, sugar.eval({ "starship", "init", shell }))
				end,
			},
			{
				target = STARSHIP_TOML,
				format = "toml",
				---@param document Document_Structured Live TOML document under patching.
				apply = function(document)
					for key, value in pairs(user_config) do
						if base[key] == nil then
							document.data[key] = value
						end
					end
				end,
			},
		},
	}
	return config
end
