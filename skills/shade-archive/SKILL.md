---
name: shade-archive
description: Retire a finished shade without destroying it — forget the jj workspaces / git worktrees it registered in the source repos so they stop cluttering them, then move the shade into the archive with its task record intact. This is retiring one shade you name or are inside; it is not deleting it (that's `shade delete`), not marking it complete (that's /shade-close), and not the cross-shade survey (that's /shade-tidy). Use to "archive this shade", "retire this shade", "put this shade away", "clean this shade out of my repo", "get these workspaces out of my repo".
argument-hint: "[shade-name] [--force]"
allowed-tools: Bash, Read, Grep, Glob
---

# Archive

You retire a shade. The point is the **source repo**: every link-mode shade
registers a jj workspace / git worktree in the repos it linked, and those pile up in
`jj workspace list` long after the work is done. Archiving forgets them, drops the
recreatable working copies, and moves the shade to `$SHADES_DIR/archived/<name>` —
keeping `TASK.md`, `LOG.md`, `DECISIONS.md`, and `tasks/`, which is exactly what
`shade delete` would have destroyed.

It is reversible: `/shade-unarchive` brings a shade back and re-attaches its
workspaces.

Lean on `AGENTS.md` for the ways of working; don't restate the protocol here.

## 1. Work out which shade

- A name in the arguments → that shade. Confirm it exists with `shade list`.
- No name → the shade containing the cwd (walk up for `AGENTS.md` + `TASK.md`). Say
  which shade you're about to archive before you do it.
- Neither → run `shade list`, show the user what's there, and ask. Don't guess.

## 2. Check it's actually finished

Archiving is cheap and reversible, so this is a sanity check rather than a hard
gate — but say what you find, and don't archive an obviously in-flight shade
without asking:

- **`DONE.md` present?** The strongest signal it's finished. If it's missing, look
  at `TASK.md`'s acceptance criteria and the tail of `LOG.md`: are there unmet
  criteria or in-flight `tasks/` units? If the shade looks live, say so and ask
  whether to archive anyway — or suggest `/shade-close` first if the work is
  genuinely done but was never closed out.
- **Unsaved work.** `shade archive` refuses when a workspace holds uncommitted or
  unpushed work, so check ahead of it and give the user the detail rather than
  letting the command fail opaquely: per repo under `repos/`, run
  `jj -R <repo> status` and `jj -R <repo> log` (or `git -C <repo> status
  --porcelain` and `git -C <repo> log --branches --not --remotes --oneline`).
  If you find something, report it and stop — offer to commit or push it first.
  Only reach for `--force` if the user explicitly accepts losing it.

## 3. Append a `LOG.md` entry first

Append an entry (timestamp via `date '+%Y-%m-%d %H:%M'`, role `archive`) recording
that the shade is being archived and why. Do this **before** archiving: once the
directory moves, the log moves with it, and an entry written afterwards means
editing a file inside the archive. Append only — never rewrite earlier entries.

## 4. Archive it

```bash
shade archive <name>          # or bare `shade archive` for the current shade
```

The command removes the shade's container, closes its herdr workspace, forgets each
host workspace in its source repo, deletes `repos/`, and moves the directory into
the archive. Clone-mode shades keep their `repos/` (independent copies can't be
recreated), so those archives are larger — mention it if that's the case.

If you are inside the shade you just archived, the cwd no longer exists: tell the
user to `cd` out (the command prints the same note).

## 5. Confirm

Report back:

- where it went (`shade list --archived` shows the archive),
- which source repos had a workspace forgotten — worth verifying with
  `jj -R <source-repo> workspace list` that the shade's workspace is gone,
- that the task record is intact and `/shade-unarchive <name>` brings it all back.
