# ADR-0007 — A role's backend and model are plugin defaults the repository overrides

*Status: in force.*

A plugin declares each role's model per backend, and `core/oat-console.toml` may choose the
console's backend. Which backend a plugin role runs on was the Run's, set by `meta fire
--agent`, unless the coordinator named one on a single `role fire`. The plugin author knows
what a role needs, but not what the repository can pay for, which backends its people have
access to, or which model does well on its code. Those facts belong to the repository, and
until now a repository could change them only by forking the plugin or by relying on every
launch to pass the right flag.

Model choices are how a team works, and so belong in plugins (ADR-0004). A repository's
override does not change that: the plugin still supplies the default, and the core only
carries the repository's replacement to the launch, as it does for a concurrency limit
(ADR-0006).

**Decision.** `.oat/roles.toml` may give any plugin role, and `oat-meta` and `oat-console`, a
`backend` and, per backend, a `model` and a `reasoning_effort`. Each field the repository sets
replaces the plugin's; each it leaves out keeps it. A launch's backend is chosen, most specific
first:

- a plugin role: `role fire --agent`, the repository's, then the Run's;
- `oat-meta`, whose backend is the Run's: `meta fire --agent`, the repository's, then Claude
  Code;
- `oat-console`: the repository's, then its plugins'. `console open --agent` is used only when
  neither chooses one, and is refused when it contradicts them.

`meta fire` settles what the repository sets into the Run, beside its limits. The console, which
belongs to no Run, reads the file each time it is opened.

**Rejected.**
- A flat `model` and `reasoning_effort` per role, applied to whatever backend the role ends up
  on. Shorter to write, but a model name belongs to one backend: the first `--agent` that
  changes a role's backend would send it a model it does not know.
- A separate file for launch settings. The repository would keep two files naming the same
  roles, and a role renamed in one would silently stop matching in the other.
- Letting `meta fire --agent` override every role's backend. It is the Run's default, not a
  statement about each role; a repository that sends one role to another backend on purpose
  would lose that choice whenever someone passed the flag.

**Consequences.**
- `meta fire --agent` is optional; a Run's backend without it is the repository's choice for
  `oat-meta`.
- A plugin can no longer assume the backend its roles run on. It still cannot today — the
  coordinator could always pass `--agent` — but the repository now makes it a standing choice.
- Changing `.oat/roles.toml` affects the next Run and the next console opened, never a Run
  already running.
- An unknown backend refuses the Run before anything is created, with `role_settings_invalid`.
