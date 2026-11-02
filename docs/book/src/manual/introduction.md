# Introduction

Confit allows us to generate reproducible userspace configuration using a simple
Lua framework. The core idea is profiles. Each profile is composed of a set of
configurations. Configurations are individual behavior units that go from a
simple text document to a complex set of config files that define a tool
configuration, including shell specific configs applied to the rc file,
binaries, structured configuration files, etc. Profiles compose those
configurations to reach diverse userspace setups.

Few use cases are:

- Distro hopping: change Linux distro and restore all your configuration with 1
  git clone and 1 confit apply. Your userspace apps, configs, shell config,
  tools, dev tools, languages, all with 1 line.
- Different profiles: have same apps with different configurations for work and
  personal time. 1 command switches apps configs, so your pc can behave like
  two.

In this book we will discover confit together creating your first configuration
with:

- your own shell rc configuration
- few cli tools included in the path and with custom aliases
- few cli tools with custom initialization
- use confit framework primitives to manually install the starship binary and
  configure the prompt as code, allowing 2 profiles for different machines to
  have different prompts

Let's go!
