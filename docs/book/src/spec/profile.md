# Profile

A profile is one Lua chunk. Its return value is one table with optional
`configs` and `documents` fields. Both fields default to empty. Any other field
fails evaluation. Confit shows the field.

```lua
return {
   configs = { shell, bat },
   documents = { sugar.rc(sugar.home(".bashrc")) },
}
```

The [documents](documents.md) page defines a document. The [patches](patches.md)
and [hooks](hooks.md) pages define the fields that a config carries.

## Profile table

| Field | Type | Default | Content |
| --- | --- | --- | --- |
| `configs` | list | empty | profile configs |
| `documents` | list | empty | documents at profile level |

A profile with neither field builds an empty manifest. A profile-level
document belongs to the profile scope. Its content still belongs to the
manifest, and its path still must be unique.

```lua
return {
   documents = {
      {
         path = route("home", ".config/starship.toml"),
         text = "add_newline = false\n",
      },
   },
}
```

A value that is not a table fails the evaluation. A field with a table but not a
list fails with the field path.

## Config table

A config groups one name with documents, patches, hooks, and requirements.

| Field | Required | Default | Content |
| --- | --- | --- | --- |
| `name` | yes | | identity from `id(name)` |
| `documents` | optional | empty | document tables |
| `patches` | optional | empty | patch tables |
| `hooks` | optional | empty | hook tables |
| `requirements` | optional | empty | target name to hint |

`name` takes the value from `id`. A plain string fails the shape. The identity
string is the name that other configs use in requirements.

```lua
local shell = {
   name = id("shell"),
   documents = {
      sugar.rc(sugar.home(".bashrc")),
   },
   patches = {
      sugar.patch(sugar.home(".bashrc"), function(document)
         table.insert(document.config, sugar.alias("ll", "ls -l"))
      end),
   },
   hooks = {
      {
         id = id("shell:reload"),
         argv = { "true" },
      },
   },
   requirements = {
      ["plugin:_/mise:install"] = "Add mise.init() to the profile.",
   },
}
```

Each optional field defaults to an empty list or map. A config with only a name
is valid. An unknown field fails evaluation.

## Order and duplicates

Config order decides where a config's
hooks start. It also decides where its patches sit among patches of the same
priority. The [patches](patches.md) page states priority.

Documents collect in this order:

1. Profile-level documents in declaration order.
2. Config documents in config order, and within a config in declaration order.

The assembled manifest then sorts documents by route. Tree members sort by
relative path.

Config names are unique. A repeated name fails with a duplicate-document error.
The log records each config that declares the name.

One document per destination. A repeated destination fails, whether the two
documents sit in one config, in two configs, or at profile level with a config.
A path comparison uses the resolved route, so the same base and relative path
collide even when the declaration text differs.

## Requirements

`requirements` is a map from a target config name to a hint string.

```lua
requirements = {
   ["plugin:_/mise:install"] = "Add mise.init() to the profile.",
}
```

The key is a config name as plain text. Every key must be a config in
the same profile. A missing target fails evaluation. The error shows the missing
config as `document '{name}'`. It ends with `config requires missing document`.
The log records the config with the requirement and any near name.

A requirement is a presence check. It confirms presence and leaves start
behavior, document merge, and declaration order to the profile.

The value is a hint string. Confit keeps it with the requirement as internal
hint text. A value that is not a string fails the shape.

## Data rules

Fields that carry JSON data accept only JSON values:

| Lua value | JSON value |
| --- | --- |
| `nil` | absent field |
| boolean | true or false |
| number | number, or null for infinite and NaN values |
| string | string |
| table with keys 1 to n | array |
| table with string keys | object |

A table with both array keys and string keys fails. An integer key outside a
1-based sequence fails. A hole in a sequence reads as null. A function, a
userdata value, and a thread fail. A table that contains itself fails.

A JSON object keeps string keys. A JSON array keeps the order of its indices.
The [documents](documents.md) page states how a structured document renders
these values.
