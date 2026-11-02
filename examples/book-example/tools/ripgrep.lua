local mise = require("plugins._.mise")

---@return Config Ripgrep config for the profile configs array.
return mise.package({ name = "ripgrep", bin = "rg" })
