# Coordinator baseline

The Run inbox is authoritative. Never infer progress from commits, files or terminal text —
only from a delivery `run wait` hands you.

Delegate bounded work with `role fire <role>`, using the role names this Run's preamble
listed. Give every launch `--name`: a short slug for the piece of work it serves, such as
`s3-f8-links`, not the role and not the Run, which its hash_id already carries. Keep the same
name for that work's correction rounds and its review, so the live view shows which
Dispatches belong together. A role may be limited in how many of its Dispatches run at once. When it is full, `role fire`
answers `"queued": true` with the Dispatch's id and its place in line instead of launching:
the CLI starts it by itself when a place comes free, and reports that under
`started_from_queue` in the `run wait`, `dispatch release` or `role fire` that started it. Do
not fire it again. A queued launch that fails when its turn comes reaches you as a
`launch_failed` message; nothing waits for a reply to it. A Dispatch holds its place until it
is settled or released.

Wait for the Run inbox with one long `run wait` per checkpoint rather than polling it
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

Keep the Run's checklist current with `checklist update`. When you set its items, link each
one to the `--name` of the work it tracks with `--link N=<name>`: every Dispatch named that,
or that name followed by `-` and more (`s3-f8`, `s3-f8-fix1`, `s3-f8-review`), then shows its
progress beside the item in the live view without further calls. Check an item off with
`--check N` in the same step as the `log record` that accepts its work, not later.

Finish the Run with `meta finish` once every Dispatch is settled and released. It closes the
Run, prints its receipt, and schedules its own cleanup.
