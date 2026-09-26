# ADR-0003 — Plugin content lands only in the Dispatch's worktree

*Status: in force.*

Both backends discover skills and agent definitions in a per-user directory. Writing a
plugin's skills and roles there before each launch breaks two promises at once. Two Runs in
two repositories that pin different versions of one plugin overwrite each other's skills
mid-Run, so an agent loads a version its Run never recorded. And plugin roles carry no
prefix (ADR-0001), so an example plugin's `worker` would silently replace a person's own
agent of that name.

**Decision.** Nothing a plugin supplies is written to a per-user directory. A role's
instructions and model travel in its session's launch — the session is the role — so no
agent definition file is written anywhere. The skills a Dispatch may load are written into
its own worktree's project-level skill directory for the selected backend and excluded
from git through the repository's local exclude file; a Dispatch sees only the skills its
role declares, plus the core skills its launch calls for. The console, which belongs to no
repository, keeps its skills in its own directory under `~/.oat/`.

**Rejected.** Keeping the per-user directory and making names unique, for example by
suffixing each skill with its plugin and version. It avoids the overwrite, but the
directory accumulates every version ever launched, every agent sees every other
repository's skills, and a role's instructions must be rewritten to use names nobody wrote.

**Consequences.**
- A person can no longer call a plugin role from their own backend session; roles exist
  only inside Runs.
- A repository that already has a project-level skill with the same name as one a plugin
  supplies cannot start that Dispatch. The launch fails and names both, rather than
  overwrite either.
- Each launch writes its skills again. They are small, and a worktree is never shared
  between two plugin versions.
