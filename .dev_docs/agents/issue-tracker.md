# Issue tracker: GitHub

Issues live in this repository's GitHub issues. Use the `gh` CLI.

- **Create**: `gh issue create --title "..." --body-file -`
- **Read**: `gh issue view <number> --comments`
- **List**: `gh issue list --state open --json number,title,labels`
- **Comment**: `gh issue comment <number> --body "..."`
- **Labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`

A spec is an issue titled `[S<n>] <topic>` with the `spec` label; its tickets are its
sub-issues, titled `[S<n>.F<m>] <topic>`, each starting with a `Spec: #<n>` line.

GitHub shares one number space between issues and pull requests; resolve a bare `#42` with
`gh pr view 42` and fall back to `gh issue view 42`.

Everything written here is public. See `AGENTS.md`.
