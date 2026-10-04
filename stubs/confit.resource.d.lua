--- @meta confit.resource

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

--- Opens one project-relative file into its handle.
--- @param path string Project-relative file path.
--- @return Handle | nil #1: The resource handle, else nil.
--- @return string | nil #2: The failure reason, else nil.
function resource(path) end


return resource
