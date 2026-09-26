# Domain docs

How agents should consume this repository's domain documentation.

## Before exploring, read these

- **`.dev_docs/CONTEXT.md`** — the vocabulary.
- **`.dev_docs/adr/`** — the records that touch the area you are about to work in.

## Use the glossary's vocabulary

When your output names a domain concept — in an issue title, a proposal, a test name — use
the term as `CONTEXT.md` defines it. Do not drift to a synonym the glossary lists under
*Avoid*. If the concept you need is not in the glossary, either you are inventing language the
project does not use, or there is a real gap: say which.

## Flag record conflicts

If your output contradicts a record, say so explicitly rather than overriding it:

> _Contradicts ADR-0001 (the core ships only oat-meta and oat-console) — but worth reopening
> because…_
