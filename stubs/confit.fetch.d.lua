--- @meta confit.fetch

--- Handle to an opened input.
--- @class Handle
local _CLASS_Handle_ = {
	--- Seal the opened input as its archive, else nil with reason.
	--- 
	--- - Non-archives produce nil with reason.
	--- @param self Handle
	--- @return Archive | nil
	--- @return string | nil
	archive = function(self) end,
	--- Read file content into its table, else nil with reason.
	--- 
	--- - Inputs without a table shape produce nil with reason.
	--- @param self Handle
	--- @return table | nil
	--- @return string | nil
	load = function(self) end,
	--- Render the opened content hash.
	--- @param self Handle
	--- @return string
	sha = function(self) end,
	--- Read file text, else nil with reason.
	--- @param self Handle
	--- @return string | nil
	--- @return string | nil
	text = function(self) end,
}

--- Handle to a verified compressed archive.
--- @class Archive
local _CLASS_Archive_ = {
	--- Open one archive member as its handle, else nil with reason.
	--- 
	--- - Names are archive-relative with forward slashes.
	--- @param self Archive
	--- @param name string
	--- @return Handle | nil
	--- @return string | nil
	member = function(self, name) end,
	--- List the archive member names, else nil with reason.
	--- 
	--- - Members are archive-relative with forward slashes.
	--- @param self Archive
	--- @return string[] | nil
	--- @return string | nil
	members = function(self) end,
}

--- @alias FetchOpts { sha256: string | nil }

--- Opens one url into its handle.
--- @param url string The url to fetch.
--- @param opts { sha256: string | nil } | nil The digest pin table.
--- @return Handle | nil #1: The resource handle, else nil.
--- @return string | nil #2: The failure reason, else nil.
function fetch(url, opts) end


return fetch
