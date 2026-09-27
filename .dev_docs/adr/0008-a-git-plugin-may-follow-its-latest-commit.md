# ADR-0008 — A git plugin may follow its latest commit

*Status: in force.*

A git plugin in `.oat/plugins.toml` was pinned to one full commit id, and trust was granted
for that commit alone. Every change to a team's plugin therefore meant, in every repository
that used it, a new pin and, on every machine, a new `plugin trust`, even for a team whose
plugin changes daily and who trusts everything its own plugin repository ships. The cost
was paid most by the people writing the plugin, who had to re-pin after every commit to try it.

**Decision.** A git pin's `commit` may be `latest` instead of a commit id. `meta fire`, `console
open` and `plugin list` ask the remote for the commit its default branch points at
(`git ls-remote <url> HEAD`) and load that commit, fetched and verified like any other. Trust for
such a pin is granted to `latest` at that URL (`plugin trust git --url <url> --commit latest`)
and covers every commit the URL will ever serve. A Run still snapshots its plugins at `meta
fire`, and `run.json` and the workflow log record the commit it actually loaded, never
`latest`.

**Rejected.**
- Pinning to a branch or tag name. `latest` says what the operator wants, the tip of the plugin,
  without naming a branch the plugin could rename; any other ref stays a commit id to pin.
- Trusting each commit `latest` resolves to. Trust would lapse on every commit to the plugin,
  which is the cost this record removes.
- Falling back to the most recently cached commit when the remote cannot be reached. A Run
  started offline would silently use an older plugin than the operator asked for; the launch
  fails instead, naming the URL.

**Consequences.**
- Whoever can push to the plugin repository's default branch decides what every Run using
  `latest` does next, on every machine that trusted it. A repository that cannot accept that
  keeps a commit id.
- Two collaborators who start Runs at different moments may run different plugin versions.
  Each Run's record says which.
- Every `meta fire` and `console open` of such a repository needs the remote to be reachable.
