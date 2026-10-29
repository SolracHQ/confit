--- @meta _
--- Build one opaque id.
--- @param name string The id text.
--- @return Id The new id.
function id(name) end

--- One opaque identity.
--- @class Id
Id = {}

--- Build one route.
--- @param base RouteBase The destination base.
--- @param relative string The relative path.
--- @return Route The new route.
function route(base, relative) end

--- Portable standard folders enum.
--- @alias RouteBase "home" | "config" | "data" | "cache" | "literal"

--- Portable route over standard folders.
--- @class Route
Route = {}

--- Return the destination base.
--- @return RouteBase The destination base.
function Route:base() end

--- Return the relative path text.
--- @return string The relative path text.
function Route:relative() end

--- Render the portable route display in sugar call form.
--- @return string The sugar call display.
function Route:display() end
