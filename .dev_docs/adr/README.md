# Architecture Decision Records

A record here captures **one** decision that is hard to reverse, is the product of a real
trade-off, and would confuse a newcomer without its context. All three, or it does not belong
here. Every record states what it costs.

## What does not belong here

| | Where it goes |
| --- | --- |
| A fact that moves with the version — command set, file formats, flags | the documentation beside the code |
| Vocabulary | `.dev_docs/CONTEXT.md` |
| Working rules for agents | `AGENTS.md` |
| Filenames, function names, test names | Nowhere. The code is the record. |

## Amendments

A record that has reached `main` is never deleted. When something changes, there are three
endings:

- **The decision still stands, but its terms changed.** Rewrite the record in place so it
  reads as the decision holds today. If the rewrite follows from another record, cite it.
- **The decision was reversed.** Write a new record. The old one gets *one line* under its
  title: `Status: superseded by ADR-NNNN.` The reason belongs in the new record.
- **It was never a decision matter.** Move it where it belongs. The record is untouched.

A record written on an epic branch is a proposal until that branch merges, and may be
rewritten freely until then.

Cite a record by number, never by filename. `.github/scripts/check-dev-docs.sh` fails when a
citation does not resolve or a record is missing from the list below.

## Current list

| ADR | Decision |
| --- | --- |
| ADR-0001 | The core owns the mechanism; how a team works enters through a plugin |
| ADR-0002 | The core ships only `oat-meta` and `oat-console`; every other role comes from a plugin |
| ADR-0003 | The core is ported without its history |
