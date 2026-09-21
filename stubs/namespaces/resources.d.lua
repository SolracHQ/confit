---@meta _
-- Editor stubs for confit.resources. The loader reads project files at runtime.
-- Shared shapes live in stubs/confit.d.lua.

---@class Resources
-- File reads namespace. Root-relative paths resolve under the project
-- root. Absolute paths resolve under the fetch cache or the extract root.
local Resources = {}

-- Reads a TOML file into a Lua table.
---@param path string # Project-root-relative path, e.g. "resources/starship.toml", or an absolute path under the fetch cache or the extract root.
---@return table
function Resources.load_toml(path) end

-- Reads a JSON file into a Lua table.
---@param path string # Project-root-relative path, or an absolute path under the fetch cache or the extract root.
---@return table
function Resources.load_json(path) end

-- Reads a YAML file into a Lua table.
---@param path string # Project-root-relative path, or an absolute path under the fetch cache or the extract root.
---@return table
function Resources.load_yaml(path) end

-- Reads a text file into a Lua string.
---@param path string # Project-root-relative path, e.g. "resources/starship.toml.j2", or an absolute path under the fetch cache or the extract root.
---@return string
function Resources.load_text(path) end

-- Reads a file into a Lua string holding raw bytes.
---@param path string # Project-root-relative path, e.g. "assets/logo.bin", or an absolute path under the fetch cache or the extract root. Absolute cache paths from fetch_file also read.
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
