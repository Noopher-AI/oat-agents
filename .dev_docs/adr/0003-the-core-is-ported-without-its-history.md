# ADR-0003 — The core is ported without its history

*Status: in force.*

The tool oat-agents came from is private. Its history holds every past version of the
organisation's own conventions and internal material, and this repository is public:
anything pushed here stays public, even after deletion.

**Decision.** No history is imported. The code is ported piece by piece, one ticket at a
time, and each piece is cleaned of the source's names and practices in the same change that
brings it in. The first version of anything in this repository is already clean. A porting
ticket names exactly which source files may be read.

**Rejected.** Importing the history through a history filter. It keeps `git blame`, but every
one of hundreds of commits — messages and diffs — would need review, one miss is a
disclosure, and the filtered history would still be full of the old names.

**Consequences.**
- The reasons behind many details were recorded only in the source's commit messages. They
  are lost unless the porting ticket carries the reason into a comment, a record or this
  repository's own commit message.
- Each porting ticket is both a move and a rename, which makes it harder to review for
  unintended behaviour changes.
