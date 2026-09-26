# ADR-0001 — The core owns the mechanism; how a team works enters through a plugin

*Status: in force.*

An agent in a Run needs two kinds of instruction: how to operate the CLI — settle a
Dispatch, read the Run inbox, report through the workflow log — and how the team it works
for does things — how branches are named, what a pull request says, how a plan is written.
When both live in the same files and are compiled into the binary, changing a convention
means changing the tool, and every team that adopts it ends up carrying a patched copy.

**Decision.** The core owns the mechanism: launching agents, worktrees, sessions, the Run
inbox, the workflow log, execution environments and the live view. Everything that
expresses how a particular team works enters through a plugin. The core is not finished
until a real team's complete working practice can be expressed as a plugin without a core
change.

**Rejected.** Letting teams adapt the core's instruction files directly. The least work up
front, but every team then maintains a diverging copy of the core, and nothing tells a
convention apart from the protocol the CLI depends on.

**Consequences.**
- A change that would encode one team's practice in the core is a missing hook. The hook is
  what gets built; the practice goes in a plugin.
- The core cannot shed a capability that an adopting team's plugin depends on without that
  plugin moving first.
- Instruction text is split in two: what the core must tell an agent about operating the CLI
  is core-owned and always present; everything else is a plugin's.
