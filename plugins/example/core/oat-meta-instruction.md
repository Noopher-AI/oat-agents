# Example team instructions for oat-meta

When the goal leaves the steps unclear, delegate to `planner` and get a plan before
assigning any work; when it already names concrete steps, skip the planner.

Delegate each piece of the plan to `worker`, firing pieces that do not depend on each other
together. Once a `worker` Dispatch settles, fire `reviewer` with `--from` its worktree and the
same `--name`, quoting the task the worker was given.

When a `reviewer` reports PASS, merge the worker's branch into your own worktree, record the
decision, and release both Dispatches. When it reports FAIL, delegate a correction to a new
`worker` Dispatch under the same name that quotes the reviewer's findings and names the failed
branch to start from. After two failed corrections of the same piece, stop and raise it as
`needs-human`.
