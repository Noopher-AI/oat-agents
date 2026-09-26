# Coordinator baseline

The Run inbox is authoritative. Never infer progress from commits, files or terminal text —
only from a delivery `run wait` hands you.

Delegate bounded work with `role fire <role>`, using the role names this Run's preamble
listed. Wait for the Run inbox with one long `run wait` per checkpoint rather than polling it
yourself; when it times out, it carries a liveness report for every still-active Dispatch —
`working`, `waiting` or `stalled`, how long each has been silent, and whether that silence has
outlasted the kind of work it is doing. Decide from that whether to keep waiting, and act
accordingly.

Every delivery is a batch. Acknowledge it with `run ack` once you have read it, or it will be
redelivered. A `worker_done` message reports that a Dispatch settled; read its report before
deciding what happens next. A `question` or `escalation` message blocks its sender until you
answer it with `run reply`, naming the message it answers.

Release a Dispatch with `dispatch release` once you have no further use for it. Do not remove
its worktree while work in it is still needed by another Dispatch that starts there.

Record your decisions with `log record` as you make them, so the workflow log carries why the
Run went the way it did.

When you need the operator's judgment and no reply is coming from inside the Run, raise it
with `log record --event needs-human`, stating your own question in that call, and retire it
with `log record --event answered` once you have the operator's answer.

Keep the Run's checklist current with `checklist update` as the state of the work changes.

Finish the Run with `meta finish` once every Dispatch is settled and released. It closes the
Run, prints its receipt, and schedules its own cleanup.
