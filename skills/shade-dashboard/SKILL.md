---
name: shade-dashboard
description: A high-level status board across all your shades at once — one numbered line per project showing where each stands (complete / active / stale / blocked), so you can see the whole portfolio in a glance. After the board, say "deeper N" to drill into that project's full status. Use to "dashboard of all my projects", "status of everything", "what's the state of all my shades", "project board", "standup".
argument-hint: [--active] [deeper N]
allowed-tools: Bash, Read, Glob
---

# Dashboard — all shades at a glance

A portfolio view across every shade, so you can see where everything stands
without opening each one. Read-only. Two modes:

1. **The board** (default) — a numbered, one-line-per-project summary.
2. **Drill-down** — when the user says `deeper N` (N is a number from the board),
   produce the full per-shade status for that one project.

## Drill-down: "deeper N"

If `$ARGUMENTS` contains `deeper <N>` (or the user says it after a board), resolve
N to the shade at that number from the most recent board and give it the full
`/shade-status` treatment: goal, progress by unit, in-flight, next up, blockers, key
decisions, recent activity. Follow the `shade-status` skill's synthesis for that shade,
and offer `--html` if they want the page. Then stop — don't re-print the board.

If there's no board in context yet, build it first (below), then drill in.

## Building the board

### 1. Enumerate shades

Run `shade list` for the names; get each path with `shade cd <name>` (it prints
the path, doesn't change directory). Assign a stable **number** to each in list
order — these are the handles the user references with "deeper N". If `--active`
is passed, drop shades that are complete (have `DONE.md`) from the board.

### 2. Pull live agent state from herdr (best-effort)

If `herdr` is available and its server is running (`herdr status` shows
`server: running`), run `herdr agent list` (JSON). Match each agent's `cwd` /
`foreground_cwd` against the shade paths to learn which shades have a **live agent**
right now and its `agent_status` (idle / busy / waiting) and `terminal_title`. This
is enrichment only — if herdr is absent, the server is down, or nothing matches,
skip it silently and build the board from files alone.

### 3. Read a cheap status per shade

For each shade, gather just enough for one line — don't do a deep read:

- `DONE.md` present? → **complete** (read its one-line summary).
- else count `tasks/*.md` statuses (skip `README.md`): any `in progress` → **active**;
  any `blocked` → **blocked**; all `done` but no `DONE.md` → **wrapping up**;
  only proposed / none → **planned**.
- last `LOG.md` entry timestamp (or file mtime) → recency; flag **stale** if nothing
  has happened in a while.
- overlay herdr state if matched: a live busy agent → **active (agent running)**.

### 4. Print the board

Lead with a one-line headline (e.g. "8 shades — 1 complete, 3 active, 2 stale, 2
planned"). Then a numbered list, most-active first (agent-running, then active,
blocked, wrapping up, planned, stale, complete). One line each:

```
3. user-identifiers   active · 3/7 units · ▲ agent busy · last log 2h ago
   → reshape user identity provenance across raven + contacts
```

Keep each entry to a title, a state, the progress fraction, any live-agent flag,
and recency — plus a short goal line pulled from `TASK.md`. End by reminding the
user they can say **"deeper N"** to expand any project, or `/shade-tidy` to clean up the
complete ones.

Never invent state — everything comes from the shades' files (and herdr, if live).
Numbers must stay consistent within a session so "deeper N" is unambiguous.
