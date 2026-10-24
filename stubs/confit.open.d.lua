--- @meta confit.open
--- Open one project-relative file.
--- @param path string Project-relative file path.
--- @return ResourceHandle? The resource handle, else nil.
--- @return string? The failure reason, else nil.
function open(path) end

--- Handle to a project or cached resource.
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

return open
