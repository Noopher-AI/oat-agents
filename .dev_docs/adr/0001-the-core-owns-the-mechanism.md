# ADR-0001 — The core owns the mechanism; how a team works enters through a plugin

*Status: in force.*

oat-agents was extracted from a tool one organisation built for itself. In that tool, the
text that tells an agent how to use the CLI and the text that tells it how that organisation
works — how branches are named, what a pull request says, how a plan is written — lived in
the same files, compiled into the binary. Changing a convention meant changing the tool.

**Decision.** The core owns the mechanism: launching agents, worktrees, sessions, the Run
inbox, the workflow log, execution environments and the live view. Everything that
expresses how a particular team works enters through a plugin. The organisation this was
extracted from will run oat-agents with a plugin of its own, and the core is not finished
until that plugin can express all of its current behaviour without a core change.

**Rejected.** Publishing a one-time open-source fork and letting both evolve separately.
Faster to release, but a plugin interface with no real user keeps the hooks that are easy to
build and misses the ones a real team needs.

**Consequences.**
- A change that would encode one team's practice in the core is a missing hook. The hook is
  what gets built; the practice goes in a plugin.
- The core cannot shed a capability that user's plugin depends on without that plugin
  moving first.
- Instruction text is split in two: what the core must tell an agent about operating the CLI
  is core-owned and always present; everything else is a plugin's.
