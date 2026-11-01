--- @meta confit.validate
--- Checks one document table.
--- @param body any The document table.
--- @return Document? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function document(body) end

--- A document with structured data in one format.
--- @class Document_Structured
--- @field path Route The destination route.
--- @field format StructuredFormat The serialization format.
--- @field data {[string]: any} Structured content for the file.

--- A destination route.
--- @class Route
--- @field base RouteBase The destination base.
--- @field relative string The relative path.
Route = {}

--- Returns the route as a sugar call like `home('.bashrc')`.
--- @return string The call text for the route.
function Route:display() end

--- A standard userspace folder.
--- @alias RouteBase "home" | "config" | "data" | "cache" | "literal"

--- A serialization format for structured data.
--- @alias StructuredFormat "json" | "toml" | "yaml"

--- A document with exact text.
--- @class Document_Text
--- @field path Route The destination route.
--- @field text string The exact file text.
--- @field mode string? The file permission mode.
--- @field unmanaged boolean? Whether the document checks the route for existence alone.

--- A document that points at another route.
--- @class Document_Link
--- @field path Route The destination route.
--- @field target Route The link target route.

--- A document with shell rc entries in three stages.
--- @class Document_Rc
--- @field path Route The destination route.
--- @field profile RcEntry[]? Entries before the guard.
--- @field config RcEntry[]? Entries after the guard.
--- @field final RcEntry[]? Entries that close the rc file.

--- One rc entry in any section.
--- @alias RcEntry { env: RcEntry_Env } | { path: RcEntry_Path } | { alias: RcEntry_Alias } | { eval: RcEntry_Eval } | { cmd: RcEntry_Cmd } | { source: RcEntry_Source }

--- A variable that the shell exports with a value.
--- @class RcEntry_Env
--- @field name string The variable name, like `EDITOR`.
--- @field value string The exported value.
--- @field when Condition? The shell session guard.

--- A condition over the shell session.
--- @alias Condition { env_eq: Condition_EnvEq } | { env_set: Condition_EnvSet } | { in_path: Condition_InPath } | { exists: Condition_Exists } | { changed: Condition_Changed } | { all: Condition[] } | { any: Condition[] } | { nop: Condition }

--- A condition that matches a shell variable against a value.
--- @class Condition_EnvEq
--- @field key string The variable name.
--- @field value string The expected value.

--- A condition that requires a shell variable to hold a value.
--- @class Condition_EnvSet
--- @field key string The variable name.

--- A condition that requires a binary on PATH.
--- @class Condition_InPath
--- @field name string The binary name.

--- A condition that requires an existing path.
--- @class Condition_Exists
--- @field route Route The path to check.

--- A condition that targets a route with pending changes.
--- @class Condition_Changed
--- @field route Route The route to check for changes.

--- A variable that gains one directory on PATH.
--- @class RcEntry_Path
--- @field name string The variable name, like `PATH`.
--- @field dir Route The directory that the entry appends.
--- @field when Condition? The shell session guard.

--- A shell alias with its expansion.
--- @class RcEntry_Alias
--- @field name string The alias name, like `ll`.
--- @field expansion string The alias expansion.
--- @field when Condition? The shell session guard.

--- A command whose output the shell evaluates.
--- @class RcEntry_Eval
--- @field argv Arg[] The command whose output runs in the shell.
--- @field when Condition? The shell session guard.

--- One command line argument.
--- @alias Arg string | Route

--- A command that the shell runs as written.
--- @class RcEntry_Cmd
--- @field argv Arg[] The command line.
--- @field when Condition? The shell session guard.

--- A file the shell sources.
--- @class RcEntry_Source
--- @field path Route The file to source.
--- @field when Condition? The shell session guard.

--- A document with bytes from a handle.
--- @class Document_Opaque
--- @field path Route The destination route.
--- @field handle ResourceHandle The opaque file handle.
--- @field mode string? The file permission mode.
--- @field unmanaged boolean? Whether the document checks the route for existence alone.

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

--- A document with a set of file members under one route.
--- @class Document_Tree
--- @field path Route The destination route.
--- @field members {[string]: TreeMember} Tree members keyed by relative path.

--- One tree member.
--- @alias TreeMember ResourceHandle | TreeMember_Full

--- A member that carries a handle and an explicit mode.
--- @class TreeMember_Full
--- @field handle ResourceHandle The file payload.
--- @field mode string The file permission mode.

--- One configuration artifact, compared as one unit.
--- @alias Document Document_Structured | Document_Text | Document_Link | Document_Rc | Document_Opaque | Document_Tree

--- Checks one config table.
--- @param config any The config table.
--- @return Config? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function config(config) end

--- One decoded config.
--- @class Config
--- @field name Id The config name.
--- @field documents Document[]? Documents in declaration order with destination paths.
--- @field patches Patch[]? Patches in profile return order.
--- @field hooks Hook[]? Hooks in profile return order.
--- @field requirements {[string]: string}? Requirements keyed by config name with hints.

--- One opaque identity.
--- @class Id
Id = {}

--- One patch table in either shape.
--- @alias Patch StructuredPatch | RcPatch | HookPatch

--- One structured patch table.
--- @class StructuredPatch
--- @field target Route? The patch target route.
--- @field filter (fun(route: Route): boolean)? The patch filter callback.
--- @field format StructuredFormat The structured format under patching.
--- @field apply fun(document: Document_Structured) The patch callback over the live document.
--- @field priority Level? The patch merge level. Higher levels apply first.
StructuredPatch = {}

--- The patch filter callback.
--- @param route Route The target route of the candidate document.
--- @return boolean
function StructuredPatch.filter(route) end

--- The patch callback over the live document.
--- @param document Document_Structured The live structured document.
function StructuredPatch.apply(document) end

--- The patch merge level.
--- @alias Level "minor" | "low" | "normal" | "high" | "major"

--- One rc patch table.
--- @class RcPatch
--- @field target Route? The patch target route.
--- @field filter (fun(route: Route): boolean)? The patch filter callback.
--- @field apply fun(document: Document_Rc) The patch callback over the live document.
--- @field priority Level? The patch merge level. Higher levels apply first.
RcPatch = {}

--- The patch filter callback.
--- @param route Route The target route of the candidate document.
--- @return boolean
function RcPatch.filter(route) end

--- The patch callback over the live document.
--- @param document Document_Rc The live rc document.
function RcPatch.apply(document) end

--- One hook patch table.
--- @class HookPatch
--- @field target string The hook id under patching.
--- @field apply fun(hook: Hook) The hook callback over the live hook.
HookPatch = {}

--- The hook callback over the live hook.
--- @param hook Hook The live hook.
function HookPatch.apply(hook) end

--- A post-config step.
--- @class Hook
--- @field argv Arg[] The command that the step runs.
--- @field path Arg[]? The PATH entries that only the subprocess sees.
--- @field id Id The hook identifier.
--- @field requires Condition? The capability condition.
--- @field when Condition? The need condition.
--- @field check Condition? The run condition.
--- @field timeout string? The run limit.

--- Checks one patch table.
--- @param patch any The patch table.
--- @return Patch? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function patch(patch) end

--- Checks one hook table.
--- @param value any The hook table.
--- @return Hook? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function hook(value) end

--- Checks one condition table.
--- @param value any The condition table.
--- @return Condition? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function condition(value) end

--- Checks one rc entry table.
--- @param value any The rc entry table.
--- @return RcEntry? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function entry(value) end

--- Checks one route handle.
--- @param value any The route handle.
--- @return Route? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function route(value) end

return {document = document, config = config, patch = patch, hook = hook, condition = condition, entry = entry, route = route}
