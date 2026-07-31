---
name: shade-graduate
description: Promote the current Claude session into a shade — when a quick exploration has grown into real, shade-worthy work. Infers the repos in play, creates a shade headlessly for them, distils the conversation so far into the shade's TASK.md / DECISIONS.md / LOG.md, seeds the first tasks/ briefs, and hands off to /shade-orchestrate. Use to "graduate this to a shade", "move this into a shade", "this has become shade-worthy", or "spin this out into a proper workspace".
argument-hint: [optional label for the shade]
allowed-tools: Bash, Read, Write, Edit, Grep, Glob
---

# Graduate

You take a session that started as a **quick exploration in an ordinary repo** and
has grown into real work, and promote it into a **shade**: a dedicated ephemeral
workspace (under `~/Code/shades/<date>-<label>`) with the repos linked in and the
context so far captured in the shade's files, ready for `/shade-orchestrate` to drive.

The value is in the **carry-over**: the user has already done thinking, made
decisions, and hit dead ends in this conversation. None of that lives on disk yet.
Your job is to distil it into `TASK.md` / `DECISIONS.md` / `LOG.md` / `tasks/` so
the shade starts already oriented, then hand off. Do not do the implementation
work here — graduate the context, then let `/shade-orchestrate` take over.

## 1. Check we're not already in a shade

If the cwd (or a parent) already has an `AGENTS.md` with a "Ways of Working"
section, you're likely already inside a shade — say so and suggest `/shade-orchestrate`
instead. Otherwise continue.

## 2. Infer the repos in play

Work out which repositories this session has actually been touching — those become
the shade's linked repos:

- Look at the cwd and any paths mentioned/edited in the conversation.
- `git -C <path> rev-parse --show-toplevel` (or walk up) to resolve each to a repo
  root; take the final path component as its name.
- Shade links repos **by name** from the configured `code_dirs`. Confirm each
  candidate is discoverable: `shade list` config aside, the reliable check is that
  the repo directory sits directly under a `code_dirs` root (e.g. `~/Code/<name>`).

Present the inferred repo list to the user and confirm it before creating anything
— getting the linked repos right matters, and it's cheap to ask.

## 3. Agree a label

Use `$ARGUMENTS` if given. Otherwise propose a short, kebab-case label from the
nature of the work (e.g. `user-identifiers`) and confirm it. Keep it terse — it
becomes the shade's directory name (prefixed with today's date automatically).

## 4. Create the shade headlessly

Create it in one shot with the headless flags (no TUI), passing each inferred repo
with `--repo`:

```bash
shade new --label <label> --repo <name> [--repo <name> ...]
```

The **last line of stdout is the shade path** — capture it; everything below is
written relative to it. Scaffolding (`AGENTS.md`, `TASK.md`, `LOG.md`,
`DECISIONS.md`, `tasks/`) is created automatically. If a repo isn't found, `shade`
bails naming it — fix the name (step 2) and retry.

## 5. Carry the context across

This is the heart of the skill. Draw **only** on what actually happened in this
conversation — do not invent scope. Write into the new shade:

- **`TASK.md`** — the north star, framed from where the exploration has landed:
  **Goal** (the outcome now being pursued, and why), **Scope / workspaces** (the
  linked repos and what each contributes), **Acceptance criteria** (concrete,
  checkable outcomes for "done"), **Out of scope**. Overwrite the skeleton.
- **`DECISIONS.md`** — every durable choice already made in the session, with its
  rationale and the alternatives that were weighed or ruled out. This is where the
  exploration's hard-won conclusions get preserved.
- **`LOG.md`** — a kickoff entry (timestamp via `date '+%Y-%m-%d %H:%M'`, role
  `graduate`) summarising what was explored before graduation, what's already been
  established, and the current state. This is the catch-up narrative for a resumed
  session.
- **`tasks/NNN-slug.md`** — if concrete next units are already clear from the
  conversation, seed the first one or two as briefs (follow `tasks/README.md`), so
  `/shade-orchestrate` has somewhere to start. Don't over-produce; a couple of real units
  beats a speculative backlog.

Keep each file scoped to its purpose and lean on `AGENTS.md` for the protocol —
don't restate the ways of working.

## 6. Hand off

Tell the user the shade is ready and where it is. Point out that the repos are
linked as fresh workspaces — **uncommitted changes in the original checkout do not
follow automatically**; if the exploration produced edits worth keeping, flag that
they need to be re-applied or committed there. Then direct them to `cd` into the
shade (`s cd <name>`) and run `/shade-orchestrate` to drive the work from the captured
context.
