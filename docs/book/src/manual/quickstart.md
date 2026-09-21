# Quickstart

You start with an empty folder and a fresh shell. This chapter builds `~/confit-demo` with one shell and one alias.

Install confit, start a project, preview a change, write it, check the files. Ten minutes from zero to managed shell lines.

## Install

Grab the release binary from the confit releases page. The asset name carries the platform. On 64 bit Linux the commands read:

```sh
curl -LO https://github.com/SolracHQ/confit/releases/latest/download/confit-linux-x64
chmod +x confit-linux-x64
mv confit-linux-x64 ~/.local/bin/confit
```

Confirm the binary runs:

```sh
confit --version
```

The output names the release:

```text
confit 0.7.0
```

## Init a project

Make a folder and scaffold a project in it:

```sh
mkdir ~/confit-demo
confit init ~/confit-demo
```

`init` writes one profile and editor stubs. It stops when the folder already holds a profile or stubs. Such folders stay untouched. It prints one line with the profile path and the file count:

```text
init: /home/you/confit-demo/profile.lua (15 files)
```

Open `profile.lua` and read it fully. It declares one shell config with one alias and one shell eval. Small file, good starting point for edits.

## First plan

Preview the change before anything lands:

```sh
confit plan ~/confit-demo/profile.lua
```

`plan` reads the profile and compares it with the files on disk. It prints the diff and a summary. It writes nothing to home paths. The summary names adds and files already in place:

```text
Bundle: 1 to add, 0 already in place.
```

It also prints the preview path and the log path. The root defaults to the profile folder, so sibling files load through `require` out of the box.

## First apply

Write the change:

```sh
confit apply ~/confit-demo/profile.lua
```

`apply` previews first, then asks. Only the literal `yes` writes. It prints the summary and the prompt:

```text
Bundle: 1 to add, 0 already in place.
Apply these changes? Type 'yes' to continue: yes
```

Type `yes` and the files land. Shell lines land in the startup files from the profile shells.

## Verify the files

The demo profile renders `bash` lines into `~/.bashrc`. Check the alias landed:

```sh
grep "alias ll=" ~/.bashrc
```

The output shows the rendered line:

```text
alias ll='ls -l'
```

Open a new shell and run `ll`.

## Where to go next

The demo works with one shell and one alias. Next, [Profile](profile.md) grows it to two shells and a shared tool file.
