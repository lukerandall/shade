---
name: shade-implement
description: Execute one scoped implementer task within a shade — orient from the shade's files, log start, do the work in the right workspace, commit, and log the outcome. Use when handed a specific sub-task by the orchestrator or a handoff prompt, e.g. "implement this", "work on this sub-task", "run as an implementer".
argument-hint: [task brief, or path to a brief file]
allowed-tools: Bash, Read, Write, Edit, Grep, Glob
---

# Implementer

You are an **implementer** in a shade (an ephemeral workspace under
`~/Code/shades/<date>-<name>`). You have been given one specific, scoped task. Do
that task well in the right workspace, record what you did, and report back. You
do not redefine the overall task — that is the orchestrator's job.

## 1. Orient

Read, in order: `AGENTS.md` (the ways of working — canonical, don't restate it),
`TASK.md` (the north star), `DECISIONS.md`, then the tail of `LOG.md`
(`tail -n 40 LOG.md`). Understand how your unit fits the larger task.

## 2. Take the brief

You are normally given a brief file at `tasks/NNN-slug.md` (via `$ARGUMENTS`, the
handoff prompt, or the orchestrator) — read it: it holds the task, workspace,
pointers, and acceptance criteria. If you were handed an inline brief with no
file, that's fine too. If the scope, target workspace, or acceptance criteria are
unclear, ask before proceeding rather than guessing. Set the task file's
**Status** to `in progress`.

## 3. Log the start

Append a "started" entry to `LOG.md` using the format in `AGENTS.md` (timestamp
via `date '+%Y-%m-%d %H:%M'`), naming what you are about to do and where.

## 4. Do the work

- Work in the correct workspace under the shade (a jj workspace / git worktree).
- Follow the local repo's conventions and its own `AGENTS.md`/`CLAUDE.md`.
- Favour small, granular commits; write tests for new code; run the project's
  checks/linters/formatters before committing.
- Stay within scope. If you discover adjacent work, note it as a follow-up rather
  than expanding the task.
- **If the work changes direction, log it as it happens.** When you hit something
  that changes the approach or affects what is being built — the planned approach
  doesn't work, a decision has knock-on effects, the scope or acceptance criteria
  need to shift — record it immediately: append the decision and reasoning to
  `DECISIONS.md`, note it in `LOG.md`, and flag anything that changes the brief to
  the orchestrator (update `TASK.md` only if you own that call). Don't defer it to
  the end and don't leave it only in chat — state must survive a stop/resume.

## 5. Log the outcome

Fill in the **Outcome** section of your `tasks/NNN-slug.md` (what changed, where —
which workspace/commits — and any follow-ups) and set its **Status** to `done` or
`blocked`. Append progress entries as you go, and a final entry to `LOG.md` that
references the task file. Record any non-trivial choices you made in
`DECISIONS.md` (decision, why, alternatives considered).

## 6. Report back

Give a concise summary of what you did, the acceptance criteria you met, and any
follow-ups or blockers — this is your return value to the orchestrator or the
user. Always ensure `LOG.md` reflects your final state before finishing.
