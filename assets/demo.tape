Output "/out/confit-demo.gif"
Set Shell "bash"
Set FontFamily "JetBrainsMono Nerd Font"
Set FontSize 14
Set Width 1280
Set Height 720
Set TypingSpeed 33ms

Type "# Today I will present you confit"
Enter
Sleep 500ms
Type "# Confit is my solution to my initial configuration problem"
Enter
Sleep 500ms
Type "# A single static binary to configure all the user space in a single command"
Enter
Sleep 500ms
Type "ls -lh /usr/local/bin/confit"
Enter
Sleep 1.5s
Type "file /usr/local/bin/confit"
Enter
Sleep 1.5s
Type "clear"
Enter
Sleep 250ms

Type "# Nothing is configured yet."
Enter
Sleep 500ms
Type "tree -a -L 2 ~"
Enter
Sleep 2.5s
Type "clear"
Enter
Sleep 250ms

Type "# Everything starts creating a dotfiles folder."
Enter
Sleep 500ms
Type "confit init /tmp/dotfiles"
Enter
Sleep 2s
Type "clear"
Enter
Sleep 250ms

Type "# It creates a bunch of files, but the most important is the main.lua profile"
Enter
Sleep 500ms
Type "cd /tmp/dotfiles"
Enter
Sleep 500ms
Type "cat main.lua"
Enter
Sleep 2.5s
Type "clear"
Enter
Sleep 250ms

Type "# Lets create a minimal profile to install bat."
Enter
Sleep 500ms
Set TypingSpeed 10ms
Type "cat > main.lua <<'EOF'"
Enter
Sleep 500ms
Type "local mise = require('plugins._.mise')"
Enter
Type "local sugar = require('plugins._.sugar')"
Enter
Enter
Type "local installer = mise.init()"
Enter
Type "local bat = mise.package({"
Enter
Type "   name = 'bat',"
Enter
Type "   rc = {"
Enter
Type "      sugar.alias('cat', 'bat'),"
Enter
Type "   },"
Enter
Type "})"
Enter
Enter
Type "return {"
Enter
Type "   configs = { installer, bat },"
Enter
Type "   documents = {"
Enter
Type "      sugar.rc(sugar.home('.bashrc')),"
Enter
Type "      sugar.rc(sugar.home('.zshrc')),"
Enter
Type "   },"
Enter
Type "}"
Enter
Type "EOF"
Enter
Set TypingSpeed 33ms
Sleep 1s
Type "# We can use check to ensure confit will create the documents we want."
Enter
Sleep 500ms
Type "confit check ."
Enter
Sleep 14s
Up
Enter
Sleep 1s
Type " "
Enter
Sleep 4s
Type "clear"
Enter
Sleep 250ms

Type "# Lets apply it."
Enter
Sleep 500ms
Type "confit apply . -f"
Enter
Sleep 6s
Type "clear"
Enter
Sleep 250ms

Type "# We can also use bundle to store the state and reuse it later."
Enter
Sleep 500ms
Type "confit bundle . -o @minimal -f"
Enter
Sleep 2.5s
Type "clear"
Enter
Sleep 250ms

Type "bash"
Enter
Sleep 1s
Type "# cat is bat now."
Enter
Sleep 500ms
Type "cat main.lua"
Enter
Sleep 2s
Type "exit"
Enter
Sleep 500ms
Type "clear"
Enter
Sleep 250ms

Type "# Lets write the starship theme."
Enter
Sleep 500ms
Set TypingSpeed 10ms
Type "cat > starship.toml <<'EOF'"
Enter
Sleep 500ms
Type "format = '$directory$git_branch$character'"
Enter
Type "add_newline = false"
Enter
Enter
Type "[directory]"
Enter
Type "style = 'cyan bold'"
Enter
Type "truncation_length = 2"
Enter
Enter
Type "[git_branch]"
Enter
Type "style = 'purple bold'"
Enter
Type "symbol = ''"
Enter
Enter
Type "[character]"
Enter
Type "success_symbol = '[>](green bold)'"
Enter
Type "error_symbol = '[x](red bold)'"
Enter
Type "EOF"
Enter
Set TypingSpeed 33ms
Sleep 1s
Type "# Lets write the work profile."
Enter
Sleep 500ms
Set TypingSpeed 10ms
Type "cat > work.lua <<'EOF'"
Enter
Sleep 500ms
Type "local mise = require('plugins._.mise')"
Enter
Type "local sugar = require('plugins._.sugar')"
Enter
Type "local nerd_fonts = require('plugins._.nerd_fonts')"
Enter
Type "local open = require('confit.open')"
Enter
Enter
Type "local installer = mise.init()"
Enter
Enter
Type "local bat = mise.package({"
Enter
Type "   name = 'bat',"
Enter
Type "   rc = {"
Enter
Type "      sugar.alias('cat', 'bat'),"
Enter
Type "   },"
Enter
Type "})"
Enter
Enter
Type "local eza = mise.package({"
Enter
Type "   name = 'eza',"
Enter
Type "   rc = {"
Enter
Type "      sugar.alias('ls', 'eza'),"
Enter
Type "      sugar.alias('ll', 'eza --long'),"
Enter
Type "   },"
Enter
Type "})"
Enter
Enter
Type "local starship = mise.package({"
Enter
Type "   name = 'starship',"
Enter
Type "   rc = {"
Enter
Type "      sugar.eval({ 'starship', 'init', 'bash' }),"
Enter
Type "   },"
Enter
Type "})"
Enter
Enter
Type "local agave = nerd_fonts.font('Agave')"
Enter
Enter
Type "return {"
Enter
Type "   configs = { installer, bat, eza, starship, agave },"
Enter
Type "   documents = {"
Enter
Type "      sugar.rc(sugar.home('.bashrc')),"
Enter
Type "      sugar.rc(sugar.home('.zshrc')),"
Enter
Type "      {"
Enter
Type "         path = sugar.config('starship.toml'),"
Enter
Type "         text = assert(assert(open('starship.toml')):text()),"
Enter
Type "      },"
Enter
Type "   },"
Enter
Type "}"
Enter
Type "EOF"
Enter
Set TypingSpeed 33ms
Sleep 1s
Type "clear"
Enter
Sleep 250ms

Type "confit apply . -p work.lua -f"
Enter
Sleep 12s
Type "clear"
Enter
Sleep 250ms

Type "# One command later, this is the new shell."
Enter
Sleep 500ms
Type "bash"
Enter
Sleep 1s
Type "ll"
Enter
Sleep 1.5s
Type "exit"
Enter
Sleep 500ms
Type "clear"
Enter
Sleep 250ms

Type "# One command switches profiles."
Enter
Sleep 500ms
Type "confit restore @minimal -f"
Enter
Sleep 6s
Type "bash"
Enter
Sleep 1s
Type "# Back to minimal."
Enter
Sleep 500ms
Type "cat main.lua"
Enter
Sleep 2s
