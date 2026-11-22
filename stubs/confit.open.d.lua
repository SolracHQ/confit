--- @meta confit.open
--- Opens one project file.
--- @param path string Project-relative file path.
--- @return ResourceHandle? The resource handle.
--- @return string? The failure reason.
function open(path) end

--- A handle to one resource.
--- @class ResourceHandle
ResourceHandle = {}

--- Returns the data reference for this handle.
--- @return ResourceRef The data reference.
function ResourceHandle:ref() end

--- Reads one file as text.
--- @return string? The decoded file text.
--- @return string? The failure reason.
function ResourceHandle:text() end

--- Loads one file into its Lua table.
--- @return table? The decoded table.
--- @return string? The failure reason.
function ResourceHandle:load() end

--- Returns the archive handle for one source.
--- @return ArchiveHandle? The archive handle.
--- @return string? The failure reason.
function ResourceHandle:archive() end

--- A reference to the data.
--- @class ResourceRef
ResourceRef = {}

--- A handle to one compressed archive.
--- @class ArchiveHandle
ArchiveHandle = {}

--- Lists the member names in archive-relative form with forward slashes.
--- @return string[]? The member names.
--- @return string? The failure reason.
function ArchiveHandle:members() end

--- Returns the handle for one archive member.
--- @param name string The member path with forward slashes.
--- @return ResourceHandle? The member handle.
--- @return string? The failure reason.
function ArchiveHandle:member(name) end

return open
