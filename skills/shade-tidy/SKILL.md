---
name: shade-tidy
description: Walk through the shades you have accumulated and clean up the finished ones. Surveys every shade, flags which are candidates for deletion (complete, stale, no unsaved work) versus which to keep (in flight or holding uncommitted/unpushed changes), and — once you choose — deletes the selected ones with `shade delete`. Use to "clean up my shades", "tidy up old projects", "which shades can I delete", "prune finished shades".
argument-hint: [--dry-run]
allowed-tools: Bash, Read, Glob
---

# Tidy shades

Shades accumulate. This skill surveys them, tells you which are safe to remove,
and does the cleanup you approve. It is destructive at the final step, so it is
careful: it never deletes without an explicit choice, and it warns loudly about
any shade holding work that isn't safely committed and pushed.

## 1. Enumerate the shades

Run `shade list` to get every shade by name. For each, get its path with
`shade cd <name>` (that command prints the path; it does not change your
directory). If there are none, say so and stop.

## 2. Gather signals for each shade

For each shade, read only what you need — don't modify anything:

- **Complete?** Is there a `DONE.md` at the shade root? Read its summary and where
  the work landed. This is the strongest "safe to delete" signal.
- **Task state.** Enumerate `tasks/*.md` (skip `README.md`) and count statuses
  (done / in progress / proposed / blocked). All done with no in-flight units is a
  completion signal even without `DONE.md`.
- **Staleness.** The timestamp of the last `LOG.md` entry (or the file's mtime) —
  how long since anything happened here.
- **Unsaved work (safety-critical).** For each linked workspace under the shade,
  check version control for anything that would be lost on delete:
  - jj: `jj -R <workspace> status` and `jj -R <workspace> log -r 'mine() & ~::@ | @'`
    (look for changes not pushed to a bookmark/remote).
  - git: `git -C <workspace> status --porcelain` and
    `git -C <workspace> log --branches --not --remotes --oneline` (unpushed commits).
  A shade with uncommitted changes or unpushed commits is **not** a safe candidate,
  regardless of `DONE.md` — surface the specifics.

## 3. Classify and present

Group the shades and show them as a scannable table (name, age, task counts,
DONE?, unsaved-work flag, one-line reason):

- **Safe to remove** — `DONE.md` present (or all units done) **and** no uncommitted
  or unpushed work in any workspace.
- **Probably done, but check** — looks finished yet has unsaved or unpushed work, or
  is complete-by-tasks but never marked `DONE.md`. Spell out exactly what's at risk.
- **Keep** — in-flight work (units in progress, recent activity) or clearly active.
- **Unclear** — missing the workflow files, or state you couldn't determine; default
  to keeping and say why.

Lead with a one-line summary (e.g. "12 shades — 5 safe to remove, 2 need a look, 5
keep"). If invoked with `--dry-run`, stop here: report only, delete nothing.

## 4. Confirm, then archive or delete

Offer both outcomes, and lead with the reversible one:

- **Archive** (`shade archive <name>`, or `/shade-archive`) — forgets the shade's
  workspaces in the source repos and moves it to the archive, **keeping** its
  `TASK.md`/`LOG.md`/`DECISIONS.md`/`tasks/`. This removes the clutter that
  accumulates in the source repos, and it's reversible with `/shade-unarchive`.
  Default to this for anything whose record might be worth having later.
- **Delete** (`shade delete <name>`) — removes the workspaces, the container, and
  the task record. Irreversible; right for shades whose record has no value.

Ask the user which shades to act on and which of the two to use — offer the "safe to
remove" set as the default, but require an explicit confirmation and let them add or
remove entries. Never assume. For any shade with unsaved or unpushed work, call it
out again and get a separate, explicit go-ahead before including it (note that
`shade archive` will itself refuse such a shade without `--force`).

Then run the approved command for each shade. Report what was archived, what was
deleted, and what was kept. Do not archive or delete the shade you are currently
inside without making that consequence clear first — in both cases the cwd goes
away.
