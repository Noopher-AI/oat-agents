# Execution environments (pods)

A role that declares `exec_environment = true` in its `role.toml` runs its build and test
commands inside a pod instead of on the host: the agent stays on the host, and each command it
sends through `oat-agents env exec -- <command>` runs in a container built from the
repository's `.devcontainer/`. Every such command is recorded in an execution ledger — its argv,
exit status, image and the exact tree it ran against — so a reviewer can tell what was verified
where.

A Run only gets pods when it has an **execution profile**. Without one, roles that asked for a
pod run their commands on the host, and oat-agents says so everywhere (see *Seeing it* below).

## Two files

| File | Says | Shared |
|---|---|---|
| `<repo>/.oat/exec.toml` | which profile this repository expects: `default_profile = "<name>"` | yes, if you commit it |
| `~/.oat/exec.toml` | how this machine provides each profile: cluster context, namespace, image build | never |

The repository file is optional. Without it, name the profile on every Run:
`oat-agents meta fire --exec-profile <name> …`.

## Setting up a machine

Scaffold a profile, then finish it by hand:

```sh
oat-agents init --repo <repo> --exec-profile local --exec-context <kube-context> --exec-namespace <namespace>
```

This writes `default_profile` into the repository file (unless one is already there) and a
`[profile.local]` table into `~/.oat/exec.toml`, followed by every optional field commented out
with what it does. A complete profile looks like:

```toml
[profile.local]
context = "my-cluster"            # the kubeconfig context to use, always written out
namespace = "agents"              # where pods are created
workspace = "hostpath"            # the worktree is mounted from the node: the pod runs on this machine
run_as_uid = "host"               # files written in the pod stay owned by you
idle_ttl = "4h"                   # `env reap` removes environments idle longer than this
ready_timeout_secs = 300

[profile.local.image]
builder = "dockerfile"            # or "devcontainer"
load = "push"                     # "none", "kind", "minikube" or "push"
registry = "registry.internal:5000"   # the address the cluster pulls by
push_to = "127.0.0.1:5000"        # the address this machine pushes to
push_tunnel = ["kubectl", "port-forward", "-n", "registry", "service/registry", "5000:5000"]
```

Then check it, in this order:

```sh
oat-agents env doctor --profile local --repo <repo>   # can this machine reach the cluster at all?
oat-agents env image  --profile local --worktree <repo>   # build and publish the image once
```

`env doctor` reads files and asks the cluster; it never builds anything. `env image` does the
real build and push, so a mistake in the `[image]` table shows up here instead of in the first
Run.

## Choosing it for a Run

`meta fire` picks the Run's profile once, and every role launched in that Run inherits it:

- `--exec-profile <name>` — this profile;
- otherwise the `default_profile` from the repository or machine file;
- `--no-exec` — no pods at all, deliberately.

## Seeing it

Nothing about pods is silent:

- **`meta fire`** returns an `exec` block: the `profile` (or `null`), where it came from
  (`--exec-profile`, `default_profile`, `--no-exec`, `none configured`), the roles that want a
  pod, and a `warning` when some of them will run on the host.
- **`role fire`** returns `exec.environment`: `pod` with its `env_id`, `pod`, `namespace` and
  `image_id`, or `host` — with a `warning` when the role asked for a pod and did not get one.
  That role's prompt says so too, so its report says where its verification ran.
- **The workflow log** records `exec_profile_selected` or `exec_profile_none` for the Run, and
  `env_skipped` for each Dispatch that ran on the host despite asking for a pod.
- **The live view** (`oat-agents tui`) shows `pod:<profile>` or `host` beside each Run, and
  `pod:<env_id>` or `HOST (no pod)` beside each agent and on its page.
- **`oat-agents env status`** lists every live environment: pod, namespace, image, Run.

## Housekeeping

- `oat-agents env status [--run R]` — what exists.
- `oat-agents env down --env <id>` — remove one environment.
- `oat-agents env reap` — remove environments left behind by Runs that ended badly, or idle past
  `idle_ttl`.

Never remove an environment another agent is still running commands in.
