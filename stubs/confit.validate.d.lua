--- @meta confit.validate
--- Checks one body table.
--- @param body any The body table under checking.
--- @return Document? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function document(body) end

--- Persisted document payload.
--- @alias Document Document_Structured | Document_Text | Document_Link | RcData | Document_Opaque | Document_Tree

--- Structured Text Document.
--- @class Document_Structured
--- @field format StructuredFormat The serialization format.
--- @field data {[string]: any} Structured content for the file.

--- Serialization format for structured documents.
--- @alias StructuredFormat "json" | "toml" | "yaml"

--- Plain Text Document.
--- @class Document_Text
--- @field text string The exact file text.
--- @field mode string? Unix permission bits.
--- @field unmanaged boolean? Existence-only document flag.

--- Symbolic Link Document.
--- @class Document_Link
--- @field target Route The link target route.

--- Portable route over standard folders.
--- @class Route
--- @field base RouteBase The destination base.
--- @field relative string The relative path.

--- Portable standard folders enum.
--- @alias RouteBase "home" | "config" | "data" | "cache" | "literal"

--- Shell RC Document.
--- @class RcData
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

--- Binary Document.
--- @class Document_Opaque
--- @field handle ResourceHandle The resource handle holding the file bytes.
--- @field mode string? Unix permission bits.
--- @field unmanaged boolean? Existence-only document flag.

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

--- File Tree Document.
--- @class Document_Tree
--- @field members {[string]: ResourceHandle} Tree members keyed by relative path.

--- Checks one config table.
--- @param config any The config table under checking.
--- @return Config? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function config(config) end

--- One decoded config.
--- @class Config
--- @field name string The config name.
--- @field documents {[Route]: Document}? Documents keyed by destination route.
--- @field patches Patch[]? Patches in profile return order.
--- @field hooks Hook[]? Hooks in profile return order.
--- @field requirements {[string]: string}? Requirements keyed by config name with hints.

--- One patch table in either shape.
--- @alias Patch StructuredPatch | RcPatch | HookPatch

--- One structured patch table.
--- @class StructuredPatch
--- @field target Route? The patch target route.
--- @field filter fun()? The patch filter callback.
--- @field format StructuredFormat The structured format under patching.
--- @field apply fun() The patch callback over current data.
--- @field priority Level? The patch merge level. Higher levels apply first.

--- Patch merge level.  - Names parse case-insensitively.
--- @alias Level "minor" | "low" | "normal" | "high" | "major"

--- One rc patch table.
--- @class RcPatch
--- @field target Route? The patch target route.
--- @field filter fun()? The patch filter callback.
--- @field apply fun() The patch callback over current sections.
--- @field priority Level? The patch merge level. Higher levels apply first.

--- One hook patch table.
--- @class HookPatch
--- @field target string The hook id under patching.
--- @field apply fun(hook: Hook) The hook callback over the live hook.

--- Face for hook patch callbacks.
--- @param hook Hook Live hook under patching.
function HookArgs(hook) end

--- One post-config step with conditions and checks.
--- @class Hook
--- @field argv Arg[] Command for the subprocess.
--- @field path Arg[]? PATH entries for the subprocess alone.
--- @field id string? The hook identifier.
--- @field requires Condition? The capability condition.
--- @field when Condition? The need condition.
--- @field check Condition? The run condition.
--- @field timeout string? The run cap in seconds.

--- Checks one patch table.
--- @param patch any The patch table under checking.
--- @return Patch? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function patch(patch) end

--- Checks one hook table.
--- @param value any The hook table under checking.
--- @return Hook? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function hook(value) end

--- Checks one condition table.
--- @param value any The condition table under checking.
--- @return Condition? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function condition(value) end

--- Checks one rc entry table.
--- @param value any The entry table under checking.
--- @return RcEntry? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function entry(value) end

--- Checks one route table.
--- @param value any The route table under checking.
--- @return Route? The decoded value, else nil.
--- @return string? The failure reason, else nil.
function route(value) end

return {document = document, config = config, patch = patch, hook = hook, condition = condition, entry = entry, route = route}
