---@meta _
-- Editor stubs for confit.resources. The loader reads project files at runtime.
-- Shared shapes live in stubs/confit.d.lua.

---@class Resources
-- File reads namespace. Reads stay jailed to the project root.
local Resources = {}

-- Reads a root-relative TOML file into a Lua table.
---@param path string # Project-root-relative path, e.g. "resources/starship.toml".
---@return table
function Resources.load_toml(path) end

-- Reads a root-relative JSON file into a Lua table.
---@param path string # Project-root-relative path.
---@return table
function Resources.load_json(path) end

-- Reads a root-relative YAML file into a Lua table.
---@param path string # Project-root-relative path.
---@return table
function Resources.load_yaml(path) end

-- Reads a root-relative text file into a Lua string.
---@param path string # Project-root-relative path, e.g. "resources/starship.toml.j2".
---@return string
function Resources.load_text(path) end

-- Reads a root-relative file into a Lua string holding raw bytes.
---@param path string # Project-root-relative path, e.g. "assets/logo.bin". Absolute cache paths from fetch_file also read.
---@return string
function Resources.load_bytes(path) end

-- Fetches a URL body into a Lua string with optional sha guarantee.
---@param url string # Remote address, e.g. "https://example.com/version".
---@param opts table|nil # Optional shape `{ sha256 = "hex" }`.
---@return string
function Resources.fetch_text(url, opts) end

-- Fetches a URL into the OS cache and returns its absolute path.
---@param url string # Remote address, e.g. "https://example.com/tool.tar.gz".
---@param opts table|nil # Optional shape `{ sha256 = "hex" }`.
---@return string
function Resources.fetch_file(url, opts) end
