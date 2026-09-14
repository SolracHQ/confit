-- solrachq.merge default plugin.
--
-- Deep merge over plain tables. `merge(base, overlay, opts?)` recurses
-- into tables, everything else last-wins. `shallow = true` merges
-- top-level keys only, replacing nested tables wholesale.
-- `list_append = true` concatenates arrays keeping every element.
-- Unknown option keys fail through confit.plugin.helpers.error.
-- Shape errors raise plain Lua errors naming the field.

-- Reports whether a table reads as a dense array.
--
-- Empty tables read as maps, matching the engine history. Integer keys
-- `1..n` with no gaps read as arrays, everything else reads as a map.
local function is_array(value)
  if type(value) ~= "table" then
    return false
  end
  local count = 0
  for key, _ in pairs(value) do
    if type(key) ~= "number" or key % 1 ~= 0 or key < 1 then
      return false
    end
    count = count + 1
  end
  if count == 0 then
    return false
  end
  for index = 1, count do
    if value[index] == nil then
      return false
    end
  end
  return true
end

-- Copies one value into a fresh table shape.
local function copy(value)
  if type(value) ~= "table" then
    return value
  end
  local out = {}
  if is_array(value) then
    for index = 1, #value do
      out[index] = copy(value[index])
    end
  else
    for key, item in pairs(value) do
      out[key] = copy(item)
    end
  end
  return out
end

-- Parses the opts table into shallow plus list_append flags.
local function parse_opts(opts)
  local shallow = false
  local list_append = false
  if opts == nil then
    return shallow, list_append
  end
  if type(opts) ~= "table" then
    error("merge: field 'opts' must be a table")
  end
  for key, _ in pairs(opts) do
    if key ~= "shallow" and key ~= "list_append" then
      confit.plugin.helpers.error("merge: unknown field '" .. tostring(key) .. "'")
    end
  end
  if opts.shallow ~= nil then
    if type(opts.shallow) ~= "boolean" then
      error("merge: field 'opts.shallow' must be a boolean")
    end
    shallow = opts.shallow
  end
  if opts.list_append ~= nil then
    if type(opts.list_append) ~= "boolean" then
      error("merge: field 'opts.list_append' must be a boolean")
    end
    list_append = opts.list_append
  end
  return shallow, list_append
end

-- Merges overlay over base into a fresh value.
local function merge_value(base, overlay, shallow, list_append, depth)
  local base_table = type(base) == "table"
  local overlay_table = type(overlay) == "table"
  if base_table and overlay_table and not is_array(base) and not is_array(overlay) then
    if not shallow or depth == 0 then
      local out = {}
      for key, item in pairs(base) do
        out[key] = copy(item)
      end
      for key, item in pairs(overlay) do
        if out[key] == nil then
          out[key] = copy(item)
        elseif shallow then
          if list_append and is_array(out[key]) and is_array(item) then
            local joined = {}
            for _, entry in ipairs(out[key]) do
              joined[#joined + 1] = copy(entry)
            end
            for _, entry in ipairs(item) do
              joined[#joined + 1] = copy(entry)
            end
            out[key] = joined
          else
            out[key] = copy(item)
          end
        else
          out[key] = merge_value(out[key], item, shallow, list_append, depth + 1)
        end
      end
      return out
    end
  end
  if list_append and is_array(base) and is_array(overlay) then
    local joined = {}
    for _, entry in ipairs(base) do
      joined[#joined + 1] = copy(entry)
    end
    for _, entry in ipairs(overlay) do
      joined[#joined + 1] = copy(entry)
    end
    return joined
  end
  return copy(overlay)
end

-- Deep-merges overlay over base into a fresh table.
--
-- Tables recurse, everything else last-wins. Neither input mutates.
local function merge(base, overlay, opts)
  if type(base) ~= "table" then
    error("merge: argument 'base' must be a table")
  end
  if type(overlay) ~= "table" then
    error("merge: argument 'overlay' must be a table")
  end
  local shallow, list_append = parse_opts(opts)
  return merge_value(base, overlay, shallow, list_append, 0)
end

return merge
