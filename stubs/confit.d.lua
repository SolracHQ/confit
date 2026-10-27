--- @meta _
--- Build one opaque id.
--- @param name string The id text under tracing.
--- @param kind? IdKind The id scope suffix.
--- @return Id The traced id.
function id(name, kind) end

--- One identity kind suffix.
--- @alias IdKind "config" | "document" | "hook"

--- One opaque identity.
--- @class Id
Id = {}
