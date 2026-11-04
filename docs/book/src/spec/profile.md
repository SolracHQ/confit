# Profile

A profile is one Lua chunk. Its return value is one table with optional
`configs` and `documents` fields. Both fields default to empty. Any other field
fails the run and shows the field.

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
| `configs` | list | empty | configs in declaration order |
| `documents` | list | empty | documents at profile level |

A profile with neither field builds an empty manifest. A profile-level document
carries no config name. Its bytes still belong to the manifest, and its path
still must be unique.

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

A value that is not a table fails the evaluation. A field with a table but
not a list fails with the field path.

## Config table

A config groups one name with documents, patches, hooks, and requirements.

| Field | Required | Default | Content |
| --- | --- | --- | --- |
| `name` | yes | | identity from `id(name)` |
| `documents` | no | empty | document tables |
| `patches` | no | empty | patch tables |
| `hooks` | no | empty | hook tables |
| `requirements` | no | empty | target name to hint |

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
is valid. An unknown field fails the run.

## Order and duplicates

Configs keep profile declaration order. That order decides where a config's
hooks run and where its patches sit among patches of the same priority. The
[patches](patches.md) page states priority.

Documents collect in this order:

1. Profile-level documents in declaration order.
2. Config documents in config order, and within a config in declaration order.

The assembled manifest then sorts documents by route. Tree members
sort by relative path.

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

The key is a config name, not an identity value. Every key must be a config in
the same profile. A missing target fails the run. The error shows the missing
config as `document '{name}'` and ends with `config requires missing document`.
The log records the config with the requirement and any near name.

A requirement is a presence check. It does not run the target, merge its
documents, or change declaration order.

The value is a hint string. The run keeps it with the requirement and does not
print it in the terminal error. A value that is not a string fails the shape.

## Data rules

Fields that carry JSON data accept only JSON values:

| Lua value | JSON value |
| --- | --- |
| `nil` | absent field |
| boolean | true or false |
| number | number, or null when not finite |
| string | string |
| table with keys 1 to n | array |
| table with string keys | object |

A table with both array keys and string keys fails. An integer key outside a
1-based sequence fails. A hole in a sequence reads as null. A function, a
userdata value, and a thread fail. A table that contains itself fails.

A JSON object keeps string keys. A JSON array keeps the order of its indices.
The [documents](documents.md) page states how a structured document renders
these values.
