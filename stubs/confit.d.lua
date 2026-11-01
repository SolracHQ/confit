--- @meta _
--- Builds one traced id.
--- @param name string The id text.
--- @return Id The new id.
function id(name) end

--- One opaque identity.
--- @class Id
Id = {}

--- Builds one traced route.
--- @param base RouteBase The destination base.
--- @param relative string The relative path.
--- @return Route The new route.
function route(base, relative) end

--- A standard userspace folder.
--- @alias RouteBase "home" | "config" | "data" | "cache" | "literal"

--- A destination route.
--- @class Route
--- @field base RouteBase The destination base.
--- @field relative string The relative path.
Route = {}

--- Returns the route as a sugar call like `home('.bashrc')`.
--- @return string The call text for the route.
function Route:display() end
