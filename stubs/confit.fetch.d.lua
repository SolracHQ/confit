--- @meta confit.fetch
--- Fetches one url.
--- @param url string The url to fetch.
--- @param sha256? string The pinned digest.
--- @return ResourceHandle? The resource handle, else nil.
--- @return string? The failure reason, else nil.
function fetch(url, sha256) end

--- A handle to a trusted resource.
--- @class ResourceHandle
ResourceHandle = {}

--- Reads one trusted file as text.
--- @return string? The decoded file text.
--- @return string? The failure reason.
function ResourceHandle:text() end

--- Loads one file into its Lua table.
--- @return table? The decoded table.
--- @return string? The failure reason.
function ResourceHandle:load() end

--- Returns the archive handle for one trusted source.
--- @return ArchiveHandle? The archive handle.
--- @return string? The failure reason.
function ResourceHandle:archive() end

--- A handle to a verified compressed archive.
--- @class ArchiveHandle
ArchiveHandle = {}

--- Returns the archive member names in archive-relative form with forward slashes.
--- @return string[]? The member names.
--- @return string? The failure reason.
function ArchiveHandle:members() end

--- Returns the handle for one archive member.
--- @param name string The member path with forward slashes.
--- @return ResourceHandle? The member handle.
--- @return string? The failure reason.
function ArchiveHandle:member(name) end

return fetch
