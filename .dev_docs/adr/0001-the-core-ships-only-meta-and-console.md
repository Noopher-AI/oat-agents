# ADR-0001 — The core ships only `oat-meta` and `oat-console`; every other role comes from a plugin

*Status: in force.*

A fixed Planner → Worker → Reviewer set is one team's way of dividing work. Another team
wants a tester, a documentation writer, or no planner at all. If the core names those roles,
every such team edits the core.

**Decision.** The core ships two roles, because the mechanism cannot run without them:
`oat-meta`, which coordinates a Run and is the only consumer of its inbox, and `oat-console`,
which observes every Run for the operator. Every other role is supplied by a plugin. Plugin
roles do not carry the `oat-` prefix — the prefix marks what the core owns. A minimal example
plugin ships `planner`, `worker` and `reviewer` so that oat-agents does something out of the
box.

**Rejected.** Shipping `oat-planner`, `oat-worker` and `oat-reviewer` in the core and letting
plugins override their instructions. It keeps the command set small, but it bakes one team's
workflow into the names and into the launch commands, and a team that divides work
differently still has to pretend it has those three roles.

**Consequences.**
- Launch commands and the meta-agent's instructions cannot name plugin roles; they refer to
  roles generically, and anything role-specific (such as launching a role inside another
  Dispatch's worktree) is a property a role declares, not a special case in the core.
- A Run cannot start without plugins: a repository chooses them when it is initialised, and
  they supply both the roles and the core roles' team instructions (ADR-0002, ADR-0004). The
  example plugin is the default choice at initialisation, so it is part of the first-run
  experience and must be kept working.
