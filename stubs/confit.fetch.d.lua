--- @meta confit.fetch

--- Resource handle over one opened input.
--- @class Handle
local _CLASS_Handle_ = {
	--- Renders the opened content hash.
	--- @param self Handle
	--- @return string
	sha = function(self) end,
	--- Reads file text, else nil with reason.
	--- @param self Handle
	--- @return string | nil
	--- @return string | nil
	text = function(self) end,
}

--- @alias FetchOpts { sha256: string | nil }

--- Opens one url into its handle.
--- @param url string The url to fetch.
--- @param opts { sha256: string | nil } | nil The digest pin table.
--- @return Handle | nil #1: The resource handle, else nil.
--- @return string | nil #2: The failure reason, else nil.
function fetch(url, opts) end


return fetch
