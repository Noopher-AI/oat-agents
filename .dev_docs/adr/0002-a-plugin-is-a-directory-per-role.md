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
└── core/                  instructions for the core roles
    ├── oat-meta-instruction.md      required from at least one plugin in a Run
    ├── oat-meta.toml                optional: skills and per-backend model
    ├── oat-console-instruction.md   required from at least one plugin in a Run
    └── oat-console.toml             optional
```

- A role's name is its directory name and is written nowhere else, so the two cannot
  disagree.
- `oat-plugin.toml` carries a format version. A plugin of an unknown version is refused with
  that reason, never half-parsed.
- Every TOML file rejects unknown fields.
- Skills refer to each other as `$skill-name`; rendering rewrites the reference for a backend
  that spells it differently.
- The core roles' team instructions come from plugins (ADR-0004) after the core baseline. Among the plugins a Run loads, at
  least one must supply each `core/*-instruction.md`; when several do, they are joined in the
  order the plugins are listed.

**Rejected.** One manifest listing every role, with instructions referenced by path. Fewer
files for a small plugin, but the manifest grows with every role, a change to one role
shows up as a change to the shared file, and a role's name lives in two places.

**Consequences.**
- A two-role plugin is already six or seven files.
- The files under `core/` are named after the core roles. A new core role means a new file
  name plugin authors have to learn, and a plugin with a `core/` file for a role that does not
  exist is refused.
- A plugin that only adds roles cannot be used on its own: some plugin in the Run has to
  supply the core roles' instructions.
- Adopting the standard skill file means a plugin's skills also work when a person installs
  them by hand, outside oat-agents.
