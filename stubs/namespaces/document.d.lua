---@meta _
-- Editor stubs for confit.document. The loader builds document tables at runtime.
-- Shared shapes live in stubs/confit.d.lua.

---@class RcOpts
---@field when? table|fun(runtime: RuntimeNs): table # Guard condition table, or a builder function receiving confit.runtime.
-- Options for confit.document.rc entry builders.
local RcOpts = {}

---@class RcSections
---@field profile? RcEntry[] # Entries rendering before the guard.
---@field config? RcEntry[] # Entries rendering after the guard.
---@field final? RcEntry[] # Entries rendering last.
-- Sections for confit.document.rc.new. Every key stays optional. Unknown keys are plan errors.
local RcSections = {}

---@class StructuredArgs
---@field path string # Destination path.
---@field data table # Data-only table.
-- Input for confit.document.structured. Unknown keys are plan errors.
local StructuredArgs = {}

---@class DocumentModeOpts
---@field mode? string # Unix mode as octal like "755" or symbolic like "rwxr-xr-x".
---@field unmanaged? boolean # Presence-only check for opaque documents; present files stay quiet.
-- Options for confit.document.text plus confit.document.opaque. Unknown keys are plan errors.
local DocumentModeOpts = {}

---@class CompressedInfo
---@field size integer # Member size in bytes.
---@field executable boolean # Executable bit from the tar mode. Map with `{mode = info.executable and "755" or "644"}`.
-- Member meta for confit.document.compressed callbacks. Use info.executable to pick the mode opt.
local CompressedInfo = {}

---@class TreeMember
---@field rel string # Destination-relative member path in manifest order.
---@field mode integer # Unix permission bits inherited from the archive member.
---@field content string # Raw member bytes.
-- Kept member for confit.document.tree documents. Profiles read
-- members to name tree files, e.g. members[1].rel.
local TreeMember = {}

---@class TreeDocument
---@field path string # Destination folder holding the members.
---@field members TreeMember[] # Kept members in relative path order.
-- Single tree document table from confit.document.tree.
local TreeDocument = {}

---@class DocumentNs
---@field rc RcNs # Rc entry table constructors namespace.
-- Document table constructors namespace.
local DocumentNs = {}

---@class RcNs
-- Rc entry table constructors namespace. Each entry goes to config:add_document.
local RcNs = {}

-- Builds a structured document table from format plus args.
---@param format string # One of "json", "toml", or "yaml".
---@param args StructuredArgs # Destination path plus data-only table.
---@return Document
function DocumentNs.structured(format, args) end

-- Builds a literal text document table.
---@param path string # Destination path.
---@param content string # Exact file text.
---@param opts DocumentModeOpts? # Optional mode, octal like "755" or symbolic like "rwxr-xr-x".
---@return Document
function DocumentNs.text(path, content, opts) end

-- Builds a symlink document table.
---@param path string # Link path.
---@param target string # Link target.
---@return Document
function DocumentNs.link(path, target) end

-- Builds an opaque document table holding raw bytes.
---@param path string # Destination path.
---@param content string # Raw file bytes, e.g. from resources.load_bytes.
---@param opts DocumentModeOpts? # Optional mode plus unmanaged flag for presence-only files.
---@return Document
function DocumentNs.opaque(path, content, opts) end

-- Unpacks one archive through a per-member callback.
---@param path string # Archive path, project-relative or fetched.
---@param callback fun(member: string, info: CompressedInfo, content: string): Document? # Keeps with a document, skips with nil.
---@return Document[] # Kept documents in archive order.
function DocumentNs.compressed(path, callback) end

-- Builds one tree document from an archive through a path picker.
---@param archive string # Archive path, project-relative or fetched.
---@param dest string # Destination folder holding the members.
---@param callback fun(member: string, info: CompressedInfo, content: string): string? # Keeps with a relative path, skips with nil.
---@return TreeDocument # Single tree document holding the manifest.
function DocumentNs.tree(archive, dest, callback) end

-- Builds the single rc document table from section lists.
---@param sections RcSections # Optional profile/config/final entry-table lists.
---@return Document
function RcNs.new(sections) end

-- Builds one alias rc entry table.
---@param name string # Alias name, e.g. "cat".
---@param value string # Alias expansion, e.g. "bat".
---@param opts RcOpts? # Optional guard.
---@return RcEntry
function RcNs.alias(name, value, opts) end

-- Builds one env rc entry table.
---@param name string # Variable name.
---@param value string # Variable value.
---@param opts RcOpts? # Optional guard.
---@return RcEntry
function RcNs.env(name, value, opts) end

-- Builds one path prepend rc entry table, defaulting to PATH.
---@param dir string # Directory, e.g. "~/.cargo/bin".
---@param opts RcOpts? # Optional guard.
---@return RcEntry
---@overload fun(var: string, dir: string, opts: RcOpts?): RcEntry
function RcNs.prepend(dir, opts) end

-- Builds one eval init rc entry table.
---@param argv string[] # Command argv evaluated as eval "$(argv...)".
---@param opts RcOpts? # Optional guard.
---@return RcEntry
function RcNs.eval(argv, opts) end

-- Builds one cmd init rc entry table.
---@param argv string[] # Command argv run as a plain line.
---@param opts RcOpts? # Optional guard.
---@return RcEntry
function RcNs.cmd(argv, opts) end

-- Builds one source init rc entry table.
---@param path string # File path sourced as source path.
---@param opts RcOpts? # Optional guard.
---@return RcEntry
function RcNs.source(path, opts) end
