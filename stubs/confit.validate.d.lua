--- @meta confit.validate

--- @alias TextDocument { mode: string | nil, text: string, unmanaged: boolean | nil }

--- @alias StructuredFormat "json" | "toml" | "yaml"

--- @alias Route { base: RouteBase, relative: string }

--- @alias RouteBase "home" | "config" | "data" | "cache" | "literal"

--- @alias StructuredDocument { data: { [string]: SerializableValue }, format: StructuredFormat }

--- @alias LinkDocument { target: Route }

--- @alias RcDocument { config: RcEntry[] | nil, final: RcEntry[] | nil, profile: RcEntry[] | nil }

--- @alias OpaqueDocument { handle: Handle, mode: string | nil, unmanaged: boolean }

--- @alias TreeDocument { members: Tree }

--- Handle to an opened input.
--- @class Handle
local _CLASS_Handle_ = {
	--- Seal the opened input as its archive, else nil with reason.
	--- 
	--- - Non-archives produce nil with reason.
	--- @param self Handle
	--- @return Archive | nil
	--- @return string | nil
	archive = function(self) end,
	--- Read file content into its table, else nil with reason.
	--- 
	--- - Inputs without a table shape produce nil with reason.
	--- @param self Handle
	--- @return table | nil
	--- @return string | nil
	load = function(self) end,
	--- Render the opened content hash.
	--- @param self Handle
	--- @return string
	sha = function(self) end,
	--- Read file text, else nil with reason.
	--- @param self Handle
	--- @return string | nil
	--- @return string | nil
	text = function(self) end,
}

--- Handle to a verified compressed archive.
--- @class Archive
local _CLASS_Archive_ = {
	--- Open one archive member as its handle, else nil with reason.
	--- 
	--- - Names are archive-relative with forward slashes.
	--- @param self Archive
	--- @param name string
	--- @return Handle | nil
	--- @return string | nil
	member = function(self, name) end,
	--- List the archive member names, else nil with reason.
	--- 
	--- - Members are archive-relative with forward slashes.
	--- @param self Archive
	--- @return string[] | nil
	--- @return string | nil
	members = function(self) end,
}

--- @alias Tree { [string]: TreeSlot }

--- @alias TreeSlot Handle | Tree

--- @alias Document TextDocument | StructuredDocument | LinkDocument | RcDocument | OpaqueDocument | TreeDocument

--- @alias StructuredPatch { apply: fun(document: StructuredDocument), format: StructuredFormat, priority: "minor" | "low" | "normal" | "high" | "major" | nil, target: Route }

--- @alias RcPatch { apply: fun(document: RcDocument), priority: "minor" | "low" | "normal" | "high" | "major" | nil, target: Route }

--- @alias Patch StructuredPatch | RcPatch

--- @alias HookFace { argv: Arg[], checks: Condition[] | nil, path: Arg[] | nil, requires: Condition | nil, timeout: string | nil, when: Condition | nil }

--- @alias SerializableValue nil | boolean | integer | number | string | table

--- @alias StrictString string

--- @alias RcEntry { env: { name: string, value: string }, when: Condition | nil } | { path: { dir: Route, name: string }, when: Condition | nil } | { alias: { expansion: string, name: string }, when: Condition | nil } | { eval: { argv: Arg[] }, when: Condition | nil } | { cmd: { argv: Arg[] }, when: Condition | nil } | { source: { path: Route }, when: Condition | nil }

--- @alias Condition { env_eq: { key: string, value: string } } | { env_set: { key: string } } | { in_path: { name: string } } | { exists: { route: Route } } | { changed: { route: Route } } | { all: Condition[] } | { any: Condition[] } | { nop: Condition }

--- @alias Arg string | Route

--- Checks one body table.
--- @param body Document The body table under checking.
--- @return boolean #1: True with nil, else false with the reason.
--- @return string | nil
function document(body) end

--- Checks one config table.
--- @param config { documents: { [Route]: Document } | nil, hooks: HookFace[] | nil, name: string, patches: StructuredPatch | RcPatch[] | nil } The config table under checking.
--- @return boolean #1: True with nil, else false with the reason.
--- @return string | nil
function config(config) end

--- Checks one patch table.
--- @param patch StructuredPatch | RcPatch The patch table under checking.
--- @return boolean #1: True with nil, else false with the reason.
--- @return string | nil
function patch(patch) end


return {document = document, config = config, patch = patch}
