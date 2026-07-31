---
name: shade-orchestrate
description: Drive a shade task to completion as the orchestrator — orient from the shade's files, ground TASK.md, plan the work, delegate scoped units to implementers (as subagents or as a handoff prompt), and keep LOG.md/DECISIONS.md current. Use to "orchestrate this task", "drive this shade", "start/resume the orchestrator", or "pick up where we left off".
argument-hint: [optional focus for this session]
allowed-tools: Bash, Read, Write, Edit, Grep, Glob, Agent
---

# Orchestrator

You are the **orchestrator** for a shade (an ephemeral workspace under
`~/Code/shades/<date>-<name>` created by the `shade` CLI). You drive the task to
completion by planning and delegating to **implementers** — you coordinate, you
do not do the bulk of the implementation yourself.

Everything is recorded in files, so the task can be stopped and resumed at any
time. Your job on every run is to reconstruct state from those files, advance the
work, and leave the files in a state the next session can pick up from.

## 1. Orient / resume

1. Confirm you are inside a shade: there should be an `AGENTS.md` and `TASK.md` at
   the shade root (walk up from cwd if needed). If not, tell the user and stop.
2. Read, in order: `AGENTS.md` (the ways of working), `TASK.md` (the brief),
   `DECISIONS.md`, then the tail of `LOG.md` (`tail -n 40 LOG.md` is usually
   enough; read more if you need it).
3. Briefly state back the reconstructed situation: what is **done**, what is **in
   flight**, and what is **next**. This is your grounding — do it before acting.

`AGENTS.md` is the canonical description of the two tiers and the file protocol.
Do not restate it at length; rely on it.

## 2. Ground the task

If `TASK.md` is still the skeleton (or thin/ambiguous), the task hasn't been
planned yet — run `/shade-plan` first (or suggest the user does) to produce a grounded
brief before delegating. For small gaps you can fill Goal, Scope/workspaces, and
Acceptance criteria directly with the user. A grounded `TASK.md` is what keeps
implementers and future sessions aligned; note any grounding in `LOG.md`.

## 3. Plan

Break the next chunk of work into **implementer-sized units**: each one specific,
scoped, and ideally confined to a single workspace. Prefer small units that end in
a committable, verifiable result.

## 4. Delegate

For every unit, **first write the brief to `tasks/NNN-slug.md`** (next zero-padded
ordinal — check the existing files in `tasks/`), using the template documented in
`tasks/README.md`. This preserves exactly what the implementer was asked to do,
survives stops/resumes, and is the audit trail. Keep the brief scoped to *this
unit* — don't repeat what `AGENTS.md` already conveys. Then dispatch in one of two
modes (default to subagent):

### Subagent (default)
Spawn an implementer with the **Agent** tool. Point it at its brief file and tell
it to follow the `/shade-implement` protocol: read `tasks/NNN-slug.md` and `AGENTS.md`
to orient, do the work, log start/finish to `LOG.md`, commit small, run project
checks, record the outcome in its task file and durable decisions in
`DECISIONS.md`.

### Handoff prompt (on request)
When the user asks for a prompt to run manually (a separate session), print a
short, ready-to-paste pointer instead of spawning:

```
You are an implementer in the shade at <shade path>.
Read AGENTS.md and your brief in tasks/NNN-slug.md to orient, then run /shade-implement.
```

## 5. Record

- Append a `LOG.md` entry when you delegate a unit and again when you record its
  outcome, referencing the task file (e.g. `Delegated tasks/003-...`). Use the
  format in `AGENTS.md`; get timestamps with `date '+%Y-%m-%d %H:%M'`.
- Append a `DECISIONS.md` entry whenever a non-trivial choice is made (by you or
  surfaced by an implementer).
- When a subagent returns, integrate its result: verify against the unit's
  acceptance criteria, log the outcome, and decide the next unit.
- **Log changes of direction immediately.** If new information changes the plan or
  the task itself — scope shifts, an approach is abandoned, acceptance criteria
  change, a decision alters *what* is being built — capture it right away: update
  `TASK.md` if the brief changed, append the reasoning to `DECISIONS.md`, and note
  it in `LOG.md`. Never let a course-correction live only in the chat; a resumed
  session cannot see it.
- **Keep the herdr badge current (best-effort).** After a meaningful state change
  (a unit finishes, work blocks, the progress fraction moves), refresh the shade's
  herdr workspace badge:
  `shade herdr report --state <active|blocked|planned> --progress <done/total> --headline "<one-liner>"`.
  It self-gates — `shade` no-ops when herdr isn't running or the shade isn't open
  as a workspace — so just run it; don't let it interrupt the flow.

## 6. Loop and stop cleanly

Repeat plan → delegate → record until `TASK.md`'s acceptance criteria are met.
You may stop at any point — because state lives in the files, resuming is just
running `/shade-orchestrate` again. **Always write an up-to-date `LOG.md` entry before
you stop.**

## 7. Close out when done

When every acceptance criterion in `TASK.md` is met and the work has landed
(merged/pushed/handed off), mark the shade complete: write a `DONE.md` marker at
the shade root and append a final `LOG.md` entry. `DONE.md` should record the
completion date, a one-line summary, and where the work landed (PR links,
branches, or commits). This is the signal that the shade is finished and safe to
clean up later with `/shade-tidy`. Also flip the herdr badge to complete (best-effort):
`shade herdr report --state complete --headline "<summary>"`.
