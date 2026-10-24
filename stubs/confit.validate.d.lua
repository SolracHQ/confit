--- @meta confit.validate
--- Check one document table, else nil with reason.
--- @param body any The document table under checking.
--- @return Document? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function document(body) end

--- Structured data file.
--- @class Document_Structured
--- @field path Route The destination route.
--- @field format StructuredFormat The serialization format.
--- @field data {[string]: any} Structured content for the file.

--- Portable route over standard folders.
--- @class Route
--- @field base RouteBase The destination base.
--- @field relative string The relative path.

--- Portable standard folders enum.
--- @alias RouteBase "home" | "config" | "data" | "cache" | "literal"

--- Serialization format for structured documents.
--- @alias StructuredFormat "json" | "toml" | "yaml"

--- Plain text file.
--- @class Document_Text
--- @field path Route The destination route.
--- @field text string The exact file text.
--- @field mode string? File mode as octal (`755`) or symbolic (`rwxr-xr-x`) text.
--- @field unmanaged boolean? Existence-only document flag.

--- Symbolic link.
--- @class Document_Link
--- @field path Route The destination route.
--- @field target Route The link target route.

--- Per-shell rc data object.
--- @class Document_Rc
--- @field path Route The destination route.
--- @field profile RcEntry[]? Entries before the guard.
--- @field config RcEntry[]? Entries after the guard.
--- @field final RcEntry[]? Entries last.

--- One rc entry in any section.
--- @alias RcEntry { env: RcEntry_Env } | { path: RcEntry_Path } | { alias: RcEntry_Alias } | { eval: RcEntry_Eval } | { cmd: RcEntry_Cmd } | { source: RcEntry_Source }

--- Plain value export.
--- @class RcEntry_Env
--- @field name string The variable name, like `EDITOR`.
--- @field value string The value under export.
--- @field when Condition? The shell session guard.

--- Shell session predicate.
--- @alias Condition { env_eq: Condition_EnvEq } | { env_set: Condition_EnvSet } | { in_path: Condition_InPath } | { exists: Condition_Exists } | { changed: Condition_Changed } | { all: Condition[] } | { any: Condition[] } | { nop: Condition }

--- Equality between a shell variable and a value.
--- @class Condition_EnvEq
--- @field key string The variable name.
--- @field value string The expected value.

--- Non-empty variable assertion.
--- @class Condition_EnvSet
--- @field key string The variable name.

--- A binary on PATH assertion.
--- @class Condition_InPath
--- @field name string The binary name.

--- A path existence assertion.
--- @class Condition_Exists
--- @field route Route The route under testing.

--- A changed document assertion.
--- @class Condition_Changed
--- @field route Route The document route under testing.

--- PATH like variable around its current value.  The directory always prepends to the existing entries.
--- @class RcEntry_Path
--- @field name string The variable name, like `PATH`.
--- @field dir Route The directory route for placement.
--- @field when Condition? The shell session guard.

--- Interactive alias.
--- @class RcEntry_Alias
--- @field name string The alias name, like `ll`.
--- @field expansion string The alias expansion.
--- @field when Condition? The shell session guard.

--- Command output evaluation through eval.
--- @class RcEntry_Eval
--- @field argv Arg[] Eval command.
--- @field when Condition? The shell session guard.

--- Text or route for command arguments.
--- @alias Arg string | Route

--- Plain command line.
--- @class RcEntry_Cmd
--- @field argv Arg[] Plain command.
--- @field when Condition? The shell session guard.

--- File source for the shell.
--- @class RcEntry_Source
--- @field path Route File route for sourcing.
--- @field when Condition? The shell session guard.

--- Raw binary file.
--- @class Document_Opaque
--- @field path Route The destination route.
--- @field handle ResourceHandle The opaque file handle.
--- @field mode string? File mode as octal (`755`) or symbolic (`rwxr-xr-x`) text.
--- @field unmanaged boolean? Existence-only document flag.

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

--- Managed file set.
--- @class Document_Tree
--- @field path Route The destination route.
--- @field members {[string]: TreeMember} Tree members keyed by relative path.

--- One tree member.  Bare handles take the default mode. Tables refer to the handle with a required mode.
--- @alias TreeMember ResourceHandle | TreeMember_Full

--- Handle with explicit mode.
--- @class TreeMember_Full
--- @field handle ResourceHandle The file payload.
--- @field mode string File mode as octal (`755`) or symbolic (`rwxr-xr-x`) text.

--- One declared document with its destination path.  One flat Lua table with the path beside body fields.
--- @alias Document Document_Structured | Document_Text | Document_Link | Document_Rc | Document_Opaque | Document_Tree

--- Check one config table, else nil with reason.
--- @param config any The config table under checking.
--- @return Config? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function config(config) end

--- One decoded config.
--- @class Config
--- @field name string The config name.
--- @field documents Document[]? Documents in declaration order with destination paths.
--- @field patches Patch[]? Patches in profile return order.
--- @field hooks Hook[]? Hooks in profile return order.
--- @field requirements {[string]: string}? Requirements keyed by config name with hints.

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
--- @param route Route Target route of the candidate document.
--- @return boolean
function StructuredPatch.filter(route) end

--- The patch callback over the live document.
--- @param document Document_Structured Live structured document under patching.
function StructuredPatch.apply(document) end

--- Patch merge level.
--- @alias Level "minor" | "low" | "normal" | "high" | "major"

--- One rc patch table.
--- @class RcPatch
--- @field target Route? The patch target route.
--- @field filter (fun(route: Route): boolean)? The patch filter callback.
--- @field apply fun(document: Document_Rc) The patch callback over the live document.
--- @field priority Level? The patch merge level. Higher levels apply first.
RcPatch = {}

--- The patch filter callback.
--- @param route Route Target route of the candidate document.
--- @return boolean
function RcPatch.filter(route) end

--- The patch callback over the live document.
--- @param document Document_Rc Live rc document under patching.
function RcPatch.apply(document) end

--- One hook patch table.
--- @class HookPatch
--- @field target string The hook id under patching.
--- @field apply fun(hook: Hook) The hook callback over the live hook.
HookPatch = {}

--- The hook callback over the live hook.
--- @param hook Hook Live hook under patching.
function HookPatch.apply(hook) end

--- One post-config step with conditions and checks.
--- @class Hook
--- @field argv Arg[] Command for the subprocess.
--- @field path Arg[]? PATH entries for the subprocess alone.
--- @field id string? The hook identifier.
--- @field requires Condition? The capability condition.
--- @field when Condition? The need condition.
--- @field check Condition? The run condition.
--- @field timeout string? The run cap in seconds.

--- Check one patch table, else nil with reason.
--- @param patch any The patch table under checking.
--- @return Patch? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function patch(patch) end

--- Check one hook table, else nil with reason.
--- @param value any The hook table under checking.
--- @return Hook? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function hook(value) end

--- Check one condition table, else nil with reason.
--- @param value any The condition table under checking.
--- @return Condition? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function condition(value) end

--- Check one rc entry table, else nil with reason.
--- @param value any The entry table under checking.
--- @return RcEntry? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function entry(value) end

--- Check one route table, else nil with reason.
--- @param value any The route table under checking.
--- @return Route? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function route(value) end

return {document = document, config = config, patch = patch, hook = hook, condition = condition, entry = entry, route = route}
