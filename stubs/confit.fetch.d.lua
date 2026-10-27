--- @meta confit.fetch
--- Open one url.
--- @param url string The url to fetch.
--- @param opts? FetchOpts The digest pin table.
--- @return ResourceHandle? The resource handle, else nil.
--- @return string? The failure reason, else nil.
function fetch(url, opts) end

--- Fetch options.
--- @class FetchOpts
--- @field sha256 string? The pinned sha256 hex digest.

--- Handle to a trusted resource.
--- @class ResourceHandle
ResourceHandle = {}

--- Read file text.
--- @return string? File text, else nil.
--- @return string? Failure reason, else nil.
function ResourceHandle:text() end

--- Read file content.
--- @return table? Decoded table, else nil.
--- @return string? Failure reason, else nil.
function ResourceHandle:load() end

--- Open the input archive.
--- @return ArchiveHandle? Archive handle, else nil.
--- @return string? Failure reason, else nil.
function ResourceHandle:archive() end

--- Handle to a verified compressed archive.
--- @class ArchiveHandle
ArchiveHandle = {}

--- List the archive member names, archive-relative with forward slashes.
--- @return string[]? Member names, else nil.
--- @return string? Failure reason, else nil.
function ArchiveHandle:members() end

--- Open one archive member.
--- @param name string Archive-relative member name.
--- @return ResourceHandle? Member handle, else nil.
--- @return string? Failure reason, else nil.
function ArchiveHandle:member(name) end

return fetch
