# Introduction

confit is a user-space configuration as code tool. It keeps user config dynamic and reproducible. You declare configs in Lua as profiles. Profiles generate bundles. You apply a bundle at any moment to switch the machine to it. Every bundle diffs against the last apply and the disk, so plan previews changes before apply writes files.

confit serves people who keep even their own configurations as configurations. People who format machines often. People who switch userspace setups per task with one command. People who hop distros and want one stable configuration.

Start with the [quickstart](quickstart.md). Run init, then plan, then apply.

This book grows one demo project from init to managed machines. Each chapter adds one piece to the same `~/confit-demo` folder. Start at [quickstart](quickstart.md) and build along.
