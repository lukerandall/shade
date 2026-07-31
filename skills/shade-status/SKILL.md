---
name: shade-status
description: Report the current status of a shade — read its TASK.md, LOG.md, DECISIONS.md, and tasks/ and summarise the goal, progress, what's in flight, what's next, key decisions, and blockers. With --html it writes a standalone status page for the project/shade. Use to "status of this shade", "where are we on this", "shade status", or "generate a status report".
argument-hint: [shade path] [--html [output.html]]
allowed-tools: Bash, Read, Write, Glob, Grep
---

# Shade status

Summarise the current state of a shade from its files — a fast "where are we" for
a task that may have been running across many sessions. Read-only by default;
`--html` additionally writes a self-contained status page.

## 1. Locate the shade

- If a path (or shade name) was given in `$ARGUMENTS`, use it. A bare name
  resolves under the shades root (e.g. `~/Code/shades/<name>`).
- Otherwise detect the shade root from the current directory: the nearest
  ancestor containing `TASK.md` and `AGENTS.md`.
- If you can't find one, say so and stop.

## 2. Read the state

Read `TASK.md`, `DECISIONS.md`, and `LOG.md`, and enumerate `tasks/*.md` (skip
`README.md`). For each unit file, extract its title, **Status**, workspace, and
whether it has an Outcome. Check for a `DONE.md` marker at the shade root — if it
exists, the shade is complete; read it for the summary and where the work landed.
Glance at `AGENTS.md` only if you need the workspace list. Don't modify anything.

## 3. Synthesise the status

Produce a concise, scannable summary covering:

- **Goal** — one or two lines from `TASK.md`.
- **Progress** — counts of units by status (done / in progress / proposed /
  blocked) out of the total, and the acceptance criteria with which are met.
- **In flight now** — units in progress and who/what holds them.
- **Next up** — the proposed/queued units.
- **Blockers & open questions** — anything blocked, plus unresolved decisions.
- **Key decisions** — the notable entries from `DECISIONS.md`.
- **Recent activity** — the last few `LOG.md` entries (most recent first).

Lead with a one-line headline (e.g. "On track — 3/7 units done, 1 in flight, no
blockers"). If `DONE.md` is present, lead with that instead ("Complete — <summary>")
and note it's a candidate for `/shade-tidy`. Reference unit files as `tasks/NNN-slug.md`
so they're easy to open.

## 4. `--html` (optional)

If `--html` is passed, also write a standalone status page:

- Output path: the argument after `--html` if given, else `status.html` in the
  shade root. Print the absolute path when done.
- **Follow the `dataviz` skill** (if available) for palette, stat tiles, and
  layout so the page reads as one system; otherwise produce a clean, self-contained
  page. Either way it must be a **single HTML file with inline CSS — no external
  assets or network dependencies** — so it can be shared or committed as-is.
- Include: the shade name and goal as a header; a row of stat tiles (units
  done/total, in progress, blocked, decisions recorded, days active); a task table
  with coloured status badges (title, status, workspace, outcome/one-liner); an
  acceptance-criteria checklist showing met vs outstanding; the key decisions; and
  a timeline built from `LOG.md`. Make it legible in both light and dark.
- Do not invent data — everything on the page comes from the shade's files. If a
  section has no data yet, show an honest empty state rather than filler.

Keep the terminal summary (step 3) as the primary output even when writing HTML.

## 5. Update the herdr badge (best-effort)

After summarising, push the headline onto the shade's herdr workspace so it shows
in herdr's workspace list:

```
shade herdr report --state <active|blocked|complete|planned> --progress <done/total> --headline "<one-liner>"
```

This is best-effort and self-gating: `shade` no-ops if herdr isn't running or the
shade isn't open as a workspace, so just run it and move on. Don't treat its output
as part of the status report.
