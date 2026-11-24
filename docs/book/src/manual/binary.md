# Binary

Now that we installed cli tools with mise, we will install the starship prompt
by hand, binary and config.

> [!NOTE]
> This is for learning purposes only. Using mise is by far better. Mise
> packages are just configs and can carry extra documents, so a starship package
> would work the same. We do it by hand here to see the framework primitives at
> work.

## The call

Our final objective in this chapter is a small extensible Lua module that
receives the configuration as a Lua table. Your profile calls it with your
settings:

```lua
local starship = require("tools.starship")({
   palette = "catppuccin",
   add_newline = false,
})
```

The call fetches the pinned release tarball and returns one config. The config
writes the binary to `~/.local/bin/starship` with mode 755. It writes your TOML
settings beside the base file. It patches each shell document with an init line
in `final`.

## The release table

Open the module. It pins the version and one asset per platform:

```lua
local STARSHIP_VERSION = "1.23.0"

local STARSHIP_PLATFORM = {
   ["linux/x86_64"] = {
      asset = "starship-x86_64-unknown-linux-gnu.tar.gz",
      sha256 = "cef41df04378c6f692913c5d9c1032d3b9a4369a1d2f3296c8300ed8838c2197",
   },
   ["linux/aarch64"] = {
      asset = "starship-aarch64-unknown-linux-musl.tar.gz",
      sha256 = "d37040138c68b5f3334bbe16d4615f0a83829deebecc5d014e779c302e8270d8",
   },
   ["macos/x86_64"] = {
      asset = "starship-x86_64-apple-darwin.tar.gz",
      sha256 = "8cac7a18fb1faf0aec50bd221dd71ecc8553850d99c212231851ae64ba11f089",
   },
   ["macos/aarch64"] = {
      asset = "starship-aarch64-apple-darwin.tar.gz",
      sha256 = "042c8001275316836a3c43fdb88d0787395edfd0c10d209e4892ab5577b80d57",
   },
}
--- more code here
```

`fact("os")` and `fact("arch")` pick the row for the host. `fetch` takes the URL
and the digest, and a mismatch fails the run. The binary comes out of the
archive with `archive:member("starship")`. `is.must` guards each step: it
returns the value or fails with the plugin name up front.

## The documents

The config carries two documents:

```lua
--- more code here
local config = {
   name = id("starship"),
   documents = {
      { path = STARSHIP_BIN, handle = binary:ref(), mode = "755" },
      { path = STARSHIP_TOML, format = "toml", data = base },
   },
--- more code here
}
```

The binary lands as an opaque document with mode 755. The TOML loads the base
file from `resources/starship.toml`. Your keys win only where the base file has
none, so the base stays readable and your choices stay visible in the call.

The shell patch uses the same pattern as zoxide. It reads the shell name from
the route and adds the init line in `final`:

```lua
--- more code here
{
   filter = function(route)
      return route.base == "home" and route.relative:match(SHELL_PATTERN) ~= nil
   end,
   apply = function(document)
      local shell = document.path.relative:match(SHELL_PATTERN)
--- more code here
      table.insert(document.final, sugar.eval({ "starship", "init", shell }))
   end,
},
--- more code here
```

The user settings land through a target patch on the TOML document. It writes
each of your keys where the base holds none:

```lua
--- more code here
{
   target = STARSHIP_TOML,
   format = "toml",
   apply = function(document)
      for key, value in pairs(user_config) do
         if base[key] == nil then
            document.data[key] = value
         end
      end
   end,
},
--- more code here
```

The file in full:

```lua
local sugar = require("plugins._.sugar")
local is = require("plugins._.is")
local open = require("confit.open")
local fetch = require("confit.fetch")
local fact = require("confit.fact")

local STARSHIP_VERSION = "1.23.0"

local STARSHIP_PLATFORM = {
   ["linux/x86_64"] = {
      asset = "starship-x86_64-unknown-linux-gnu.tar.gz",
      sha256 = "cef41df04378c6f692913c5d9c1032d3b9a4369a1d2f3296c8300ed8838c2197",
   },
   ["linux/aarch64"] = {
      asset = "starship-aarch64-unknown-linux-musl.tar.gz",
      sha256 = "d37040138c68b5f3334bbe16d4615f0a83829deebecc5d014e779c302e8270d8",
   },
   ["macos/x86_64"] = {
      asset = "starship-x86_64-apple-darwin.tar.gz",
      sha256 = "8cac7a18fb1faf0aec50bd221dd71ecc8553850d99c212231851ae64ba11f089",
   },
   ["macos/aarch64"] = {
      asset = "starship-aarch64-apple-darwin.tar.gz",
      sha256 = "042c8001275316836a3c43fdb88d0787395edfd0c10d209e4892ab5577b80d57",
   },
}

local STARSHIP_MEMBER = "starship"

local STARSHIP_RESOURCE = "resources/starship.toml"

local SHELL_PATTERN = "^%.(%a+)rc$"

local STARSHIP_BIN = route("home", ".local/bin/starship")
local STARSHIP_TOML = route("config", "starship.toml")

return function(user_config)
   local platform = fact("os") .. "/" .. fact("arch")
   local release =
      is.must("starship: ", STARSHIP_PLATFORM[platform], "unsupported platform '" .. platform .. "'")
   local url = "https://github.com/starship/starship/releases/download/v"
      .. STARSHIP_VERSION
      .. "/"
      .. release.asset
   local archive_handle =
      is.must("starship: ", fetch(url, release.sha256), "cannot fetch the release tarball")
   local archive = is.must("starship: ", archive_handle:archive(), "cannot open the release tarball")
   local binary =
      is.must("starship: ", archive:member(STARSHIP_MEMBER), "the release tarball holds no " .. STARSHIP_MEMBER)
   local resource = is.must("starship: ", open(STARSHIP_RESOURCE), "cannot open " .. STARSHIP_RESOURCE)
   local base = is.must("starship: ", resource:load(), "cannot load " .. STARSHIP_RESOURCE)

   local config = {
      name = id("starship"),
      documents = {
         { path = STARSHIP_BIN, handle = binary:ref(), mode = "755" },
         { path = STARSHIP_TOML, format = "toml", data = base },
      },
      patches = {
         {
            filter = function(route)
               return route.base == "home" and route.relative:match(SHELL_PATTERN) ~= nil
            end,
            apply = function(document)
               local shell = document.path.relative:match(SHELL_PATTERN)
               if not shell then
                  error("starship: route '" .. document.path.relative .. "' holds no shell name")
               end
               table.insert(document.final, sugar.eval({ "starship", "init", shell }))
            end,
         },
         {
            target = STARSHIP_TOML,
            format = "toml",
            apply = function(document)
               for key, value in pairs(user_config) do
                  if base[key] == nil then
                     document.data[key] = value
                  end
               end
            end,
         },
      },
   }
   return config
end
```

## Check and apply

`confit check` shows the binary and the new totals:

```text
+ home('.local/bin/starship'): opaque
Summary
Documents: 6 to add, 0 to change, 0 to destroy.
Hooks: 1 to add, 0 to change, 0 to destroy.
```

A digest line follows the opaque header. A new release changes the hash and
prints an update line instead.

```sh
confit apply ~/dotfiles
```

```text
applied: 6 files, 0 removed
restore: confit restore %1
```

The binary lands at `~/.local/bin/starship`. The same config patches both shell
documents with `eval "$(starship init bash)"` and `eval "$(starship init zsh)"`
in `final`. `confit restore %1` undoes this run.
