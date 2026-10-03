--- @meta confit.fetch

--- Resource handle over one sealed input.
--- 
--- Text answers file bytes with reason pairs, sha answers the sealed
--- content hash.
--- @class ResourceHandle
local _CLASS_ResourceHandle_ = {
	--- Reads the sealed content hash.
	--- @param self ResourceHandle
	--- @return string
	sha = function(self) end,
	--- Reads file text, else nil with reason.
	--- @param self ResourceHandle
	--- @return string | nil
	--- @return string | nil
	text = function(self) end,
}

--- @alias FetchOpts { sha256: string | nil }

--- Seals one url into its handle.
--- @param url string The url to fetch.
--- @param opts { sha256: string | nil } | nil The digest pin table.
--- @return ResourceHandle | nil #1: The resource handle, else nil.
--- @return string | nil #2: The failure reason, else nil.
function fetch(url, opts) end


return fetch
