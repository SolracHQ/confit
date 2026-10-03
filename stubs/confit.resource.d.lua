--- @meta confit.resource

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

--- Seals one resource-relative file into its handle.
--- @param path string Resource-relative file path.
--- @return ResourceHandle | nil #1: The resource handle, else nil.
--- @return string | nil #2: The failure reason, else nil.
function resource(path) end


return resource
