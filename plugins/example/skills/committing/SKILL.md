# Committing

Commit on the branch your worktree is already on. The meta-agent finds your work by that
branch, so never create, switch or rename a branch, and never push.

- Commit only what the task needs; leave unrelated files as they were.
- Write the subject in the imperative, under 72 characters, saying what changed
  (`Rate-limit failed login attempts`), and add a body only for the why.
- Commit everything before you settle: uncommitted work is lost when the Dispatch is released.
- Finish with `git status` clean and quote the commit ids in your report.
