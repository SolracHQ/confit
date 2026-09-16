-- solrachq.template default plugin.
--
-- Template rendering over engine atoms. `template(path, opts)` reads the
-- `src` file with `confit.resources.load_text`, renders it with
-- `confit.utils.render` plus the `vars` table, then returns a plain text
-- document for the destination path. Unknown option keys fail through
-- confit.plugin.helpers.error. Only existing primitives compose this
-- module.

-- Parses the opts table into src plus vars.
local function parse_opts(opts)
  if type(opts) ~= "table" then
    confit.plugin.helpers.error("template: field 'opts' must be a table")
  end
  for key, _ in pairs(opts) do
    if key ~= "src" and key ~= "vars" then
      confit.plugin.helpers.error("template: unknown field '" .. tostring(key) .. "'")
    end
  end
  local src = opts.src
  if type(src) ~= "string" or src == "" then
    confit.plugin.helpers.error("template: field 'src' must be a non-empty string")
  end
  local vars = opts.vars
  if vars == nil then
    vars = {}
  end
  if type(vars) ~= "table" then
    confit.plugin.helpers.error("template: field 'vars' must be a table")
  end
  return src, vars
end

-- Renders one template file into a plain text document.
--
-- Reads `opts.src` root-relative, renders with `opts.vars`, then builds
-- the text document for `path`.
local function template(path, opts)
  if type(path) ~= "string" or path == "" then
    confit.plugin.helpers.error("template: field 'path' must be a non-empty string")
  end
  local src, vars = parse_opts(opts)
  local text = confit.resources.load_text(src)
  local rendered = confit.utils.render(text, vars)
  return confit.document.text(path, rendered)
end

return template
