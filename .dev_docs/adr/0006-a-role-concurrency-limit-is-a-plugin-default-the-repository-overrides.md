# ADR-0006 — A role's concurrency limit is a plugin default the repository overrides, and the CLI queues past it

*Status: in force.*

A Run can start as many Dispatches of one role as its coordinator fires. Each one is a
worktree, an agent session, often an execution environment. Past some number, the machine,
the cluster or the budget can no longer keep up. The number depends on the role: a role that
builds and tests costs more than one that reads. It also depends on where the Run happens: the
same plugin is used in repositories whose builds differ by orders of magnitude.

ADR-0004 gives plugins every coordination policy and refuses to let `meta fire` carry one.
A concurrency limit is not that kind of policy. It does not change how a team works; it
bounds what the machine is asked to hold at once. It is enforced where the CLI decides
whether an agent exists, which ADR-0004 gives the CLI.

**Decision.** A plugin role's `role.toml` may declare `max_concurrent`, the plugin's default
limit on how many of that role's Dispatches one Run runs at once. A repository replaces it for
any plugin role in `.oat/roles.toml`. `meta fire` settles each role's effective limit once and
records it in the Run. A Dispatch holds a place from launch until it is settled or released.
A `role fire` for a role at its limit is queued, not refused. The CLI launches queued fires
oldest first from the coordinator's own commands (`role fire`, `run wait`,
`dispatch release`) as places come free. A queued launch that then fails reaches the
coordinator as a Run inbox message. One still queued when the Run finishes is dropped and
logged.

**Rejected.**
- Refusing a `role fire` over the limit and leaving the retry to the coordinator. It needs no
  queue, but every coordinator then has to track the free places and fire again at the right
  moment. The limit becomes a rule each plugin's coordinator must be taught to follow, rather
  than a bound the CLI keeps.
- Limits in the plugin only. The plugin author cannot know the repository's build cost or the
  machine it runs on.
- Limits in the repository only. A plugin could not ship a sensible default for a role it
  knows to be expensive.
- One limit across every Run of a repository. It fits the machine more closely, but Runs would
  hold each other up. `role fire` would have to read every open Run, and a Run's behaviour
  would depend on Runs it knows nothing about.
- Launching queued fires from `dispatch done`. That command runs in the settling Dispatch's own
  session, and the coordinator may release that session, killing the launch partway.

**Consequences.**
- `role fire` has two answers: a launched Dispatch, or a queued launch with the id its Dispatch
  will carry. The coordinator baseline has to say so, or a coordinator fires the same work
  twice.
- A place freed while the coordinator runs none of its commands stays empty until it does.
  Settlement always reaches the coordinator through `run wait`, so the delay is only as long as
  the coordinator's own turn.
- A queued Fresh-start Dispatch branches from the coordinator's worktree as it is when the
  Dispatch launches, not when it was fired.
- Changing `.oat/roles.toml` or a plugin's default affects the next Run, never one already
  running.
- A repository can set any number but cannot lift a plugin's limit to none.
