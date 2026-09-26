# ADR-0004 — The CLI owns the agent; the plugin owns its instructions

*Status: in force.*

Every agent in a Run is told two different kinds of thing. Some of it is a fact about this
launch that only the CLI knows and that breaks the Run if it is wrong: which Run the session
is bound to, which command delegates, which command ends the Dispatch or the Run, that nobody
is watching the terminal. The rest is how to do the job well: when to delegate, how long to
wait, what to do about an agent that has gone quiet, how to ask a person, when to give up.
The second kind is where teams differ, and it is where a team's hard-won practice lives.

**Decision.** The CLI owns every agent: whether it exists, how it is launched, which Run it is
bound to, how it ends, and a preamble generated in code that states those facts. No plugin
can change or remove the preamble. Everything else an agent is told — for plugin roles and
for `oat-meta` and `oat-console` alike — is instruction text a plugin supplies. The core ships
no instruction text of its own.

**Rejected.** Shipping core instructions for `oat-meta` and `oat-console` — the inbox loop,
what each liveness state calls for, a limit on correction rounds, a `--max-review-rounds`
flag — and letting plugins append to them. It gives a usable coordinator with no plugin, but
it puts one way of coordinating into every installation and leaves a team that coordinates
differently arguing with the core's text in its own additions.

**Consequences.**
- A Run cannot start unless its plugins supply instructions for both core roles
  (ADR-0002). The example plugin has to supply them, and a coordinator is only as good as
  the plugin that instructs it.
- The preamble is the only text the core can rely on every agent having read. A fact the CLI
  needs every agent to know goes there, or it is not guaranteed to be known.
- `meta fire` carries no coordination policy. A limit on retries or review rounds is a
  plugin's instruction or a Big Plan's text.
