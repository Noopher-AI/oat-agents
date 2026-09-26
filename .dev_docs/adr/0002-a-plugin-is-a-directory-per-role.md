# ADR-0002 — A plugin is a directory with one directory per role

*Status: in force.*

A plugin format is a promise to every plugin author: once published, changing it breaks
their plugins. It has to be readable by someone who has never seen oat-agents, and a mistake
in it has to surface when the plugin is loaded, not when an agent is launched hours into a
Run.

**Decision.** A plugin is a directory:

```
<plugin>/
├── oat-plugin.toml        format version, name, description
├── roles/<role>/
│   ├── role.toml          description, start location, execution environment,
│   │                      prior verification, skills, per-backend model
│   └── instructions.md
├── skills/<skill>/
│   ├── SKILL.md           the standard skill file both backends read
│   └── …                  any supporting files; scripts keep their mode
└── core/                  optional additions to the core roles
    ├── oat-meta.md        appended after oat-meta's core text
    ├── oat-meta.toml      skills and per-backend model for oat-meta
    ├── oat-console.md
    └── oat-console.toml
```

- A role's name is its directory name and is written nowhere else, so the two cannot
  disagree.
- `oat-plugin.toml` carries a format version. A plugin of an unknown version is refused with
  that reason, never half-parsed.
- Every TOML file rejects unknown fields.
- Skills refer to each other as `$skill-name`; rendering rewrites the reference for a backend
  that spells it differently.

**Rejected.** One manifest listing every role, with instructions referenced by path. Fewer
files for a small plugin, but the manifest grows with every role, a change to one role
shows up as a change to the shared file, and a role's name lives in two places.

**Consequences.**
- A two-role plugin is already six or seven files.
- The files under `core/` are named after the core roles. A new core role means a new file
  name plugin authors have to learn, and a plugin with a `core/` file for a role that does not
  exist is refused.
- Adopting the standard skill file means a plugin's skills also work when a person installs
  them by hand, outside oat-agents.
