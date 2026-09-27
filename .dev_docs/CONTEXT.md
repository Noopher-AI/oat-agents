# oat-agents

A command-line tool that runs a supervised team of coding agents on one machine: a
coordinator takes one Big Plan and delegates bounded work to other agents, each in its own
git worktree and tmux session, settling the results through a file-based inbox. The core
supplies the mechanism and two roles; plugins supply every other role and the way a team
works.

## How to read this glossary

A word earns a place here only if getting it wrong causes a concrete mistake. Every *Avoid*
entry carries the reason it is dangerous. A word is defined once; a second definition is a
defect.

## Language

### Work

**Big Plan**
The one input a Run is started from: what is to be done, written for the coordinator.
*Avoid*: **task list** — the coordinator decides the steps; a Big Plan states the outcome.

**Run**
One execution of one Big Plan, from `meta fire` until the coordinator finishes it. Owns its
Dispatches, its Run inbox and its workflow log.
*Avoid*: **job**, **session** — a session is one agent's terminal; a Run spans many of them.

**Dispatch**
One launch of one role inside a Run: one agent session, one worktree, one settlement.
*Avoid*: **task** — several Dispatches can serve the same piece of work (a correction round
is a new Dispatch), and conflating them loses which attempt said what.

**hash_id**
The stable identifier of one agent execution, `<role>-<4 hex>-<name>`, quoted in every log
line about it. `<name>` is what the Dispatch is for — its `role fire --name`, or the branch of
the worktree it starts in — and the Run's name only for the coordinator and for a launch that
named nothing.
*Avoid*: deriving a hash_id anywhere but from the Dispatch's record — a copy that uses a
different name looks for a session that does not exist.

**Queued launch**
A `role fire` for a role already at its concurrency limit in the Run, waiting for a place. It
already has the id its Dispatch will carry, but it is not a Dispatch yet: it has no worktree and
no session, and the CLI launches it when a place comes free.
*Avoid*: firing it again — it will start on its own, and a second fire queues a second copy of
the same work.

**Settle**
What an agent does to end its own Dispatch with an outcome and a report. A Dispatch that is
not settled is not finished, whatever its terminal shows.

### Communication

**Run inbox**
The Run's queue of messages from its agents to the coordinator — questions, escalations and
completion reports. It has exactly one consumer: that Run's `oat-meta`.
*Avoid*: **reading the inbox** for anything that merely observes — a delivery is handed over
once, and an observer that takes one leaves the coordinator waiting for a message that is
already gone. Observers read the workflow log.

**Delivery**
One batch handed to the coordinator by `run wait`, acknowledged as a whole.

**Workflow log**
The append-only record of everything that happened in a Run, written by the CLI and by the
coordinator's decisions. The only place an observer reads a Run from.

### Roles

**Role**
A named kind of agent: its instructions, the skills and MCP servers it may use, and its model
settings.
*Avoid*: **subagent** — a role runs as its own session in its own worktree, not inside
another agent's turn; treating it as a subagent leads to relaying work that was meant to
be delegated.

**Core role**
A role the core ships and owns: `oat-meta` and `oat-console`. Only core roles carry the
`oat-` prefix.

**Plugin role**
Any role supplied by a plugin, such as `planner`, `worker` or `reviewer` in the example
plugin. The core knows nothing about what a plugin role does.
*Avoid*: assuming a fixed Planner → Worker → Reviewer set in the core — which roles exist is
a plugin's decision.

**oat-meta**
The coordinator of one Run. It delegates, reads the Run inbox, and finishes the Run; it
does not do the Big Plan's work itself.

**oat-console**
The operator's system console. It belongs to no Run and observes all of them; it never
consumes a Run inbox and never settles or releases a Dispatch.

### Extension

**Plugin**
A unit that supplies roles, skills and conventions to the core without changing it. Its
format is decided by the first spec.
*Avoid*: **customization** for changes made to the core itself — a customization that
needs a core edit is a missing plugin hook, and naming it so is how the hook gets built.

**MCP server**
A stdio process declared by a plugin and exposed to a backend session only when that
session's role binds it. The Run or console session's plugin snapshot fixes its launch
configuration.
*Avoid*: assuming every role, or the backend's other sessions, can see a plugin's MCP server.

**Skill**
A versioned instruction file an agent loads on demand, synchronized to the selected
backend's skill directory before a launch.

**Backend**
The agent program a session runs: currently Claude Code or Codex.
*Avoid*: **runtime** — it reads as where a Dispatch's commands run, which is the execution
environment, not the program that runs the agent.

### Execution

**Worktree**
A git worktree the core creates for one Dispatch, branched from its coordinator's worktree,
or, for a role that starts in an existing worktree and is fired with `--at`, from that commit.

**Execution environment**
A container, described by an execution profile, in which a Dispatch's build and test
commands run instead of the host.
On Kubernetes, the only provider so far, it is one pod, and the operator-facing surfaces (the
live view, `docs/exec-environments.md`) call it that. A role that asked for one and ran on the
host is reported as such, never left to be inferred.

**Execution profile**
A named description of how a machine provides execution environments. The repository names
the profile it expects; the machine says how it provides it.

## Words not to use

| Word | Why |
|---|---|
| **agent** alone, where a role or a Dispatch is meant | "The agent failed" does not say whether a role is wrong or one attempt failed. |
