---
name: shade-refine
description: Record a mid-flight change of direction in a shade whose plan is already grounded — reconcile TASK.md, DECISIONS.md, LOG.md, and the in-flight tasks/ briefs so the shift survives a stop and resume. This revises an existing plan and captures what it supersedes; it is not grounding a plan from nothing (that's /shade-kickoff) and it does not do the implementation. Use when "the plan changed", "the scope changed", "we're changing direction", "re-scope this shade", "a new constraint came up", "we're abandoning that approach", or "update the task".
allowed-tools: Bash, Read, Write, Edit, Grep, Glob
---

# Refine

The plan changed after work already started. You do the **bookkeeping** of that
change so a resumed session sees the new reality, not the old one — you update the
living brief, record what the change supersedes, and reconcile the in-flight units.
You do **not** implement the new work here; you leave the shade ready for
`/shade-orchestrate` to drive it.

This is the counterpart to `/shade-kickoff`: kickoff grounds a `TASK.md` from a
blank slate, whereas refine mutates an *already-grounded* plan and keeps an honest
supersession trail of what it replaces. `AGENTS.md` describes this under "Capturing
changes of direction" — lean on it; don't restate the ways of working at length.

## 1. Orient

Confirm you are inside a shade: walk up from cwd for an `AGENTS.md` (with a "Ways of
Working" section) and a `TASK.md` at the shade root. If you can't find them, say so
and stop — there is no grounded plan to refine.

Read `TASK.md`, `DECISIONS.md`, and the tail of `LOG.md`, and skim `tasks/` for the
units in flight, so the refinement is anchored in where the work actually stands.

## 2. Understand the change

Establish with the user *what* changed and *why*: a new constraint, an approach
that's been abandoned, a change to scope or acceptance criteria, or any decision
that changes *what* is being built. If it's ambiguous — or you can't tell which
existing criteria or units it invalidates — ask before you edit anything. A
refinement recorded against a misunderstanding is worse than none.

## 3. Update `TASK.md` in place

`TASK.md` is the living north star, **not** append-only. Edit it so it describes the
new reality: adjust the Goal, Scope, and Acceptance criteria; add, reword, or remove
criteria as the change demands. Do not leave superseded criteria standing as if they
still hold — a resumed session trusts this file.

## 4. Append a `DECISIONS.md` entry

Append (never rewrite) a new decision capturing: the decision, its rationale, the
alternatives weighed, and — crucially — **what it supersedes**, referencing the
prior decision or the criteria it replaces. This is the durable, honest trail of how
the plan got here.

## 5. Append a `LOG.md` entry

Append an entry (timestamp via `date '+%Y-%m-%d %H:%M'`, role `refine`) noting the
shift, so the timeline reflects when and why the direction changed. Append only.

## 6. Reconcile `tasks/`

For pending or in-flight briefs that the change invalidates, mark them superseded or
obsolete **in the task file** (note what changed and why) — don't delete them; the
history is the point. Where the new direction makes the next unit obvious, you may
seed a fresh `tasks/NNN-slug.md` (next zero-padded ordinal) so the orchestrator has a
starting point. Do **not** implement the work here.

## 7. Refresh the herdr badge (best-effort)

If the change moved the progress denominator (units added or dropped), refresh it:

```bash
shade herdr report --state <active|blocked|planned> --progress <done/total> --headline "<one-liner>"
```

It self-gates — `shade` no-ops when herdr isn't running or the shade isn't open as a
workspace — so just run it and don't let it interrupt the refinement.

## 8. Hand back

Summarise what you changed (which criteria moved, which units were superseded, what
the new decision records) and tell the user to run `/shade-orchestrate` to continue
driving from the refined plan.
