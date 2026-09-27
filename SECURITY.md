# Security Policy

## Reporting a vulnerability

Please do not report security vulnerabilities in public issues, discussions, or pull requests.
Instead, use GitHub's private vulnerability reporting form on the repository's
[**Security** tab](https://github.com/Noopher-AI/oat-agents/security/advisories/new).

Include the affected version or commit, reproduction steps or a proof of concept, the potential
impact, and any suggested mitigation. We will acknowledge the report as soon as practical and keep
you updated as we investigate and prepare a fix.

Please allow the maintainers a reasonable opportunity to address the vulnerability before public
disclosure.

## Scope

oat-agents launches coding agents with the permissions of the user who runs it, and plugin
instructions steer those agents. Reports are especially welcome for:

- a plugin, repository configuration or Run inbox file that makes oat-agents run a command or
  write outside the worktree it was given, without the operator having trusted it;
- a way around plugin trust (`oat-agents plugin trust`);
- `install.sh` fetching or running something other than what the operator asked for.

What an agent does once you have launched it with a trusted plugin is governed by that agent's
own permission model, not by oat-agents.

## Supported versions

oat-agents is currently pre-1.0. Security fixes are made on the `main` branch and included in the
next release. Older releases are not maintained separately unless a release note explicitly says
otherwise.
