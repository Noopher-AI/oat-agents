# ADR-0009 — A plugin MCP server is bound to a role launch

*Status: proposed.*

A plugin may depend on a local tool exposed through an MCP server, but giving every session
every server creates accidental access and makes one role's needs a backend-wide setting.
Writing plugin configuration into the user's Codex or Claude configuration would also let one
repository change tools for unrelated sessions and Runs.

**Decision.** `oat-plugin.toml` may declare named stdio MCP servers with a command and optional
arguments and environment values. A plugin role or core role lists the server names it needs in
its own `mcp_servers` field. A binding may refer only to a declaration in that same plugin;
unknown or repeated bindings and duplicate server names across plugins are load errors. The Run
or console snapshot fixes the commands and values used by its launches.

The CLI passes a role's resolved servers only to that role's selected backend launch. It does
not mutate backend-wide configuration. Codex gets a per-launch configuration override with an
OAT-scoped key associated with the Dispatch or console session; Claude Code gets a launch-local
MCP config and strict MCP scoping. A
role without a binding receives no plugin MCP server. `plugin validate` parses and reports this
configuration without trusting the plugin or launching a command.

**Rejected.**
- Giving every server declared by every Run plugin to every role. Most roles need no such tool,
  and ambient access makes it impossible to review which role can affect or disclose data.
- Installing plugin servers into the user's global backend settings. That escapes the Run's
  plugin snapshot and can affect unrelated repositories and sessions.
- Letting a role bind a server declared by another plugin. It couples two independently pinned
  plugins and makes changing one plugin silently change the other's role capabilities.

**Consequences.**
- A plugin author must declare a server and bind each role that uses it. A server declaration
  alone is inert.
- A person trusting a plugin also trusts the local executable it names and the literal values
  passed to it. Plugin validation never executes that command.
- The initial format supports stdio servers only. Backend configuration is created per launch,
  and environment values are local plugin contents rather than a secret store.
- Existing plugins remain valid because the new manifest and role fields are optional.
