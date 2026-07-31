---
name: shade-close
description: Close out a finished shade — verify its acceptance criteria are met and the work has landed, then write the DONE.md completion marker, append a final LOG.md entry, and flip the herdr badge to complete. This is finishing an existing shade you are inside, not promoting a session into one (that's /shade-promote) and not cleaning up already-finished shades across the board (that's /shade-tidy). Use to "close this shade", "mark this shade done/complete", "wrap up this shade", or "finish this shade".
allowed-tools: Bash, Read, Write, Edit, Grep, Glob
---

# Close

You close out the shade you are currently in: the task is finished, so you record
that fact durably. The point of a dedicated skill is the **guard** — you do not
rubber-stamp completion. You verify the criteria are genuinely met and the work has
landed before writing the marker, and you never overwrite an existing close-out.

Lean on `AGENTS.md` for the ways of working and the meaning of `DONE.md`; don't
restate the protocol at length here.

## 1. Orient

Confirm you are inside a shade: walk up from cwd for an `AGENTS.md` (with a "Ways of
Working" section) and a `TASK.md` at the shade root. If you can't find them, say so
and stop — there is nothing to close.

Read `TASK.md`'s acceptance criteria and the tail of `LOG.md` to reconstruct where
the work stands.

## 2. Guard before closing

Do not close on request alone — check first, and stop if anything is outstanding:

- **Acceptance criteria** — walk each criterion in `TASK.md` and confirm it is
  actually met (from the log, the files, and the repos), not merely intended.
- **Work has landed** — the work must be merged, pushed, or otherwise handed off.
  Check the linked repos (under `repos/`, or the shade's repo entries) for
  uncommitted or unpushed changes, e.g. `jj status` / `jj log` (or `git status` /
  `git log @{u}..`) in each. Surface anything you find.

If any criterion is unmet or work is unlanded, **report exactly what's outstanding
and stop** — do not write `DONE.md`. Closing is only for a genuinely finished shade.

## 3. Write `DONE.md`

If a `DONE.md` already exists at the shade root, the shade is already closed — say
so and stop; **never overwrite it**.

Otherwise write `DONE.md` at the shade root with:

- the completion date (via `date '+%Y-%m-%d %H:%M'`),
- a one-line summary of what was accomplished,
- where the work landed — PR links, branches, or commit ids.

## 4. Append a final `LOG.md` entry

Append an entry (timestamp via `date '+%Y-%m-%d %H:%M'`, role `close`) noting that
the shade is complete and pointing at `DONE.md`. Append only — never rewrite earlier
entries.

## 5. Flip the herdr badge (best-effort)

Run:

```bash
shade herdr report --state complete --headline "<one-line summary>"
```

It self-gates — `shade` no-ops when herdr isn't running or the shade isn't open as
a workspace — so just run it and don't let it interrupt the close-out.

Then tell the user the shade is closed and where the work landed. Once `DONE.md`
exists, the shade is safe to clean up later with `/shade-tidy`.
