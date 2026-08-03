---
name: shade-unarchive
description: Bring an archived shade back to life — pick one from the archive, move it back to the active shades directory, and re-attach the jj workspaces / git worktrees it had in its source repos, so work can resume where it stopped. Use to "unarchive a shade", "bring back a shade", "resurrect that project", "revive an old shade", "restore a shade", "pick up that archived project again", or when work needs to resume on something previously retired with /shade-archive.
argument-hint: "[shade-name]"
allowed-tools: Bash, Read, Grep, Glob
---

# Unarchive

You bring a retired shade back. `/shade-archive` forgot its host workspaces and
dropped the recreatable working copies; you reverse that — move the shade back to
`$SHADES_DIR/<name>` and re-attach a workspace per repo recorded in `shade.toml`,
from the `primary_repo_path` saved there. The task record was never touched, so once
it's back the resume protocol picks up exactly where it left off.

Lean on `AGENTS.md` for the ways of working; don't restate the protocol here.

## 1. Pick the shade

```bash
shade list --archived
```

- A name in the arguments → use it (check it's in that list).
- No name → show the archive as a **numbered list** so the user can pick one. Make
  it easy to choose: for each, read the archived `TASK.md` (its Goal line) and the
  last `LOG.md` entry, and give a one-line summary plus when it was last touched.
  Paths are `$SHADES_DIR/archived/<name>` — get `$SHADES_DIR` from `shade config
  path`'s `env_dir`, or just read the paths from the listing. If the archive is
  empty, say so and stop.

## 2. Bring it back

```bash
shade unarchive <name>
```

That moves the directory back and re-attaches one workspace per repo in
`shade.toml`, printing a line per repo. Read that output — a repo whose source has
since moved or been deleted is **skipped, not fatal**: the shade still comes back,
just without that workspace. Surface any skip clearly and offer to re-link it (the
source path is in `shade.toml`; `shade new --repos` from inside the shade can link a
repo afresh).

The command prints the restored path last, so `s unarchive <name>` also `cd`s you
there.

## 3. Re-orient

The shade is live again, so reconstruct its state rather than assuming: read
`AGENTS.md` → `TASK.md` → `DECISIONS.md` → the tail of `LOG.md`, and check `tasks/`
for units that were in flight when it was archived.

Note anything the archive/restore cycle may have changed underneath the work:

- **`DONE.md` present?** Then this shade was already closed — ask what's reopening
  it before treating the task as live.
- **The re-attached workspaces are fresh.** They sit at whatever the source repo's
  default is now, not at the commit the shade was on. If `LOG.md` or `tasks/` name
  the commits or bookmarks the work was on, say so — the work may need `jj new
  <change>` / `git checkout <branch>` to get back to it.
- **Untracked files are gone.** Anything the VCS wasn't tracking (a local `.env`,
  build output) did not survive archiving. Flag it if setup is likely needed —
  `shade.toml`'s `shade_setup` is the usual way back.

## 4. Append a `LOG.md` entry and hand off

Append an entry (timestamp via `date '+%Y-%m-%d %H:%M'`, role `unarchive`) noting
that the shade was brought back, which workspaces re-attached, and anything skipped.
Append only.

Then summarise where the shade stands and what the obvious next step is — usually
`/shade-orchestrate` to resume the work, or `/shade-refine` first if the plan needs
to change before it restarts.
