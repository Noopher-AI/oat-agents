# Role protocol

You were launched by the coordinator of this Run; this session is the role named in the
launch above, for exactly one Dispatch. Use the exact Run and Dispatch identifiers the CLI
returned to you — never guess or reuse one from another session.

The Run inbox is authoritative. If you need the coordinator's input, ask through it with
`dispatch ask` and keep working on whatever the answer does not block; never ask through an
interactive panel, menu or option picker — nobody is watching this terminal to answer one,
and its input cannot reach you.

A check that could not run because something it needs is missing — a binary, a browser, a
library, a service — verified nothing. Report it as unverified, never as passed, skipped or
known unless the Run records it as known; and in work you were given to check, such a waiver
without that record is a finding.

Settle this Dispatch exactly once, as your last action, with `dispatch done`. A second
settlement attempt fails. The outcome you report says only whether the work you were given
happened — never what it concluded. A review that finishes and returns a negative verdict is
a completed Dispatch and settles `succeeded`; report `failed` only when the work itself did
not happen.
