# Troubleshooting

You stand at a demo that applies safely. This chapter fixes common stops.

Three common stops and their fixes.

## A link stands where a file wants to land

**Symptom.** Apply reports wrote, the link stays, and the disk reads strange.

**Cause.** A symlink stands where a text document wants to land.

**Fix.** Delete the link and re-run apply. This is a known limitation with a planned fix.

## A hook fails

**Symptom.** Apply stops and later steps never run.

**Cause.** A hook tool exits with a failure code.

**Fix.** Read the run log, fix the tool, and re-run. The log holds one header line per hook and its output:

```text
hook 1 of 1: tool --flag
```

The log path prints after the run. Steps with passing checks stay skipped.

## A hand edit drifts

**Symptom.** The next plan reports the file changed on disk and asks again.

**Cause.** A hand edit moved the disk past the last apply.

**Fix.** Answer `yes` to overwrite the disk with the planned content. Any other answer writes nothing and keeps the hand edit:

```text
vanished: manually deleted. changed outside config: add to config or the next apply loses them
```

For the exact contract see [spec output](../spec/output.md).

Common stops now have fixes. Next, [Reference](reference.md) lists every command shape.
