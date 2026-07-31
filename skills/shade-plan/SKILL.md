---
name: shade-plan
description: Turn a rough idea into a grounded TASK.md for a shade — explore the linked workspaces, interrogate the idea, draft the goal/scope/acceptance criteria, and seed the log. The front door before /shade-orchestrate. Use to "plan this task", "scope this out", "kick off a task", "help me figure out what to build", or when TASK.md is still empty.
argument-hint: [rough description of the idea]
allowed-tools: Bash, Read, Write, Edit, Grep, Glob, Agent
---

# Plan

You turn a rough idea into a **grounded `TASK.md`** — the brief that `/shade-orchestrate`
and implementers then drive towards. This is the front door: it runs once at the
start of a task (and again if the task needs re-framing). Do not implement here;
produce a plan the user signs off on.

## 1. Orient

Confirm you are inside a shade (there is an `AGENTS.md` at the root; walk up from
cwd if needed). Read `AGENTS.md` for the ways of working, and note which
repos/workspaces are linked into the shade — those are the surface area you're
planning against. If `TASK.md`/`DECISIONS.md`/`LOG.md` already have content, read
them first; you may be re-planning, not starting fresh.

## 2. Understand the idea

Take the user's rough description (`$ARGUMENTS` or the conversation). Ask focused
clarifying questions until you can state, concretely: what outcome they want, why,
what "done" looks like, and any hard constraints or non-goals. Don't over-ask —
prefer a few high-leverage questions over a long form.

## 3. Explore to ground it

Survey the linked workspaces (read-only) to root the plan in reality: relevant
files and existing patterns, how similar things are done here, and whether the
idea is feasible as imagined. For a broad or multi-repo surface, use the **Agent**
tool to explore in parallel. Surface anything that reshapes the plan.

## 4. Draft the plan

Write `TASK.md` with concrete, checkable content:

- **Goal** — the outcome and why.
- **Scope / workspaces** — which repos are in play and what each contributes.
- **Acceptance criteria** — specific, verifiable outcomes that mean "done".
- **Out of scope** — what you're deliberately not doing.

Optionally sketch the first few implementer-sized units to show the shape of the
work, but keep the heavy planning of *how* — and the writing of per-unit briefs
into `tasks/` — for the orchestrator.

## 5. Review and sign off

Walk the user through the draft and iterate until they're happy. `TASK.md` is the
north star — it's worth getting right before work starts.

## 6. Seed the record and hand off

- Append a kickoff entry to `LOG.md` (timestamp via `date '+%Y-%m-%d %H:%M'`).
- Record any framing decisions (chosen approach, key trade-offs) in `DECISIONS.md`.
- Tell the user to run `/shade-orchestrate` to start driving the task.
