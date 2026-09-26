# ADR-0005 — A console belongs to one repository

*Status: in force.*

The console is the operator's agent for asking about the system. It needs team instructions
like any other agent (ADR-0004), and plugins reach a Run only through a repository's
`.oat/plugins.toml`. A console that belongs to no repository has nowhere to take its plugins
from, and one that watches every Run on the machine would judge a Run governed by one team's
plugins by another team's rules.

**Decision.** A console is opened from an initialised repository and belongs to it. It uses
that repository's plugins, resolved, trust-checked and snapshotted exactly as for a Run. There
is one console per repository; it works in its own directory under `~/.oat/consoles/`, never
inside the repository, so what its skills write stays out of the repository and survives a
restart. It observes only its repository's Runs. The live view itself still lists every Run.

**Rejected.**
- A console-only plugin declaration under `~/.oat/`. It is a machine-level plugin source,
  and the console should be governed by a repository's rules like everything else.
- A repository's console observing every Run on the machine. Useful for spotting a stuck Run
  elsewhere, but it applies one repository's rules to another's Runs.

**Consequences.**
- An operator watching several repositories opens one console per repository.
- A console cannot be opened outside an initialised repository.
- The console reads its repository through paths, not as its working directory.
