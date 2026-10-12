--- @meta confit.fetch
--- Opens one url into its handle.
--- @param url string The url to fetch.
--- @param opts? FetchOpts The digest pin table.
--- @return ResourceHandle? The resource handle, else nil.
--- @return string? The failure reason, else nil.
function fetch(url, opts) end

--- Fetch options.
--- @class FetchOpts
--- @field sha256 string? The pinned sha256 hex digest.

--- Handle to a project resource.
--- @class ResourceHandle
ResourceHandle = {}

--- Read file text, else nil with reason.
--- @return string?
--- @return string?
function ResourceHandle:text() end

--- Render the opened content hash.
--- @return string
function ResourceHandle:sha() end

--- Read file content into its table, else nil with reason.  - Inputs without a table shape produce nil with reason.
--- @return table?
--- @return string?
function ResourceHandle:load() end

--- Open the input as its archive, else nil with reason.  - Non-archives produce nil with reason.
--- @return ArchiveHandle?
--- @return string?
function ResourceHandle:archive() end

--- Handle to a verified compressed archive.
--- @class ArchiveHandle
ArchiveHandle = {}

--- List the archive member names, else nil with reason.  - Members are archive-relative with forward slashes.
--- @return string[]?
--- @return string?
function ArchiveHandle:members() end

--- Open one archive member as its handle, else nil with reason.  - Names are archive-relative with forward slashes.
--- @param name string
--- @return ResourceHandle?
--- @return string?
function ArchiveHandle:member(name) end

return fetch
