use super::generate_id;
use crate::env::integration::ExecEnvironments;
use crate::env::state::EnvStore;
use crate::environment::Environment;
use crate::error::{codes, err};
use crate::launch::{self, LaunchSpec};
use crate::plugins::snapshot::catalog_from_snapshot;
use crate::role::{Backend, RoleCatalog, RoleDefinition, StartLocation};
use crate::store::{RunRecord, Store};
use crate::worktree;
use anyhow::Result;
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Subcommand, Debug)]
pub enum RoleCommand {
    Fire(FireArgs),
}

#[derive(Args, Debug)]
pub struct FireArgs {
    pub role: String,
    #[arg(long)]
    pub from: Option<PathBuf>,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub agent: Option<String>,
    #[arg(long)]
    pub trust_workspace: bool,
    #[arg(long)]
    pub prompt: Option<String>,
    #[arg(long)]
    pub input_file: Option<PathBuf>,
}

pub fn run(command: RoleCommand, env: &dyn Environment, exec: &dyn ExecEnvironments) -> Result<Value> {
    match command {
        RoleCommand::Fire(args) => fire(args, env, exec),
    }
}

fn fire(args: FireArgs, env: &dyn Environment, exec: &dyn ExecEnvironments) -> Result<Value> {
    let run_id = env
        .var("OAT_RUN_ID")
        .ok_or_else(|| err(codes::RUN_NOT_BOUND, "role fire runs inside a Run; OAT_RUN_ID is not set"))?;
    let task = crate::resolve_text_input(&args.prompt, &args.input_file)?;

    let store = Store::open(env)?;
    let run_record: RunRecord = store.load_run(&run_id)?;
    let repo = PathBuf::from(&run_record.repo);

    let agent = args.agent.clone().unwrap_or_else(|| run_record.backend.clone());
    let backend = Backend::from_str(&agent)?;

    // The Run's plugins were resolved and trust-checked once, at `meta fire` (F5's Architecture:
    // "Snapshot"); every role launch inside it rebuilds its catalog from that copy rather than
    // re-reading `.oat/plugins.toml` or re-checking trust.
    let catalog = catalog_from_snapshot(&store.run_plugin_snapshot_dir(&run_id))?;
    let catalog = &catalog;
    let role_def = catalog.role(&args.role)?;

    match (role_def.start, &args.from) {
        (StartLocation::Fresh, Some(_)) => {
            return Err(err(
                codes::INVALID_ROLE_OPTION,
                format!("role '{}' starts fresh; --from is not accepted", args.role),
            ))
        }
        (StartLocation::Existing, None) => {
            return Err(err(
                codes::ROLE_SOURCE_REQUIRED,
                format!("role '{}' starts in an existing worktree; --from is required", args.role),
            ))
        }
        _ => {}
    }

    let launch_name = args.name.clone().unwrap_or_else(|| generate_id(&args.role));
    let dispatch_id = generate_id("dispatch");

    let (worktree_path, branch, created_fresh) = match role_def.start {
        StartLocation::Fresh => {
            let branch = worktree::branch_name(&run_record.name, &launch_name);
            let path = worktree::worktree_path(&repo, &run_record.name, &launch_name);
            let meta_worktree = run_record
                .meta_worktree
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| repo.clone());
            let base_commit = worktree::current_commit(&meta_worktree)?;
            worktree::create_worktree(&repo, &path, &branch, &base_commit)?;
            (path, branch, true)
        }
        StartLocation::Existing => {
            let from = args.from.clone().unwrap();
            let canonical = from
                .canonicalize()
                .map_err(|_| err(codes::INVALID_WORKTREE_ID, format!("no such worktree: {}", from.display())))?;
            if !worktree::is_worktree_of(&repo, &canonical)? {
                return Err(err(
                    codes::INVALID_WORKTREE_ID,
                    format!("'{}' is not a worktree of this Run's repository", canonical.display()),
                ));
            }
            let branch_output = worktree::run_git(&canonical, &["rev-parse", "--abbrev-ref", "HEAD"])?;
            let branch = String::from_utf8_lossy(&branch_output.stdout).trim().to_string();
            (canonical, branch, false)
        }
    };

    if backend == Backend::Claude {
        let _ = launch::write_claude_local_settings(&worktree_path);
        if args.trust_workspace {
            let _ = launch::trust_claude_workspace(env, &worktree_path);
        }
    }

    let model = role_def.models.get(&backend).cloned().unwrap_or_default();

    let log = crate::event_log::EventLog::open(env);
    let mut skills = role_def.skills.clone();
    let mut instructions = vec![role_def.instructions.clone()];
    let env_binding = bring_up_environment(env, &log, &run_id, role_def, &args.role, &worktree_path, exec)?;
    if let Some(record) = &env_binding {
        skills.push(crate::env::exec_environment_skill());
        instructions.push(render_execution_environment_block(record));
        if role_def.prior_verification {
            let reusable = exec.reusable(env, &worktree_path, record);
            instructions.push(render_prior_verification_block(record, &reusable));
        }
    }

    let spec = LaunchSpec {
        run: run_record.clone(),
        role_label: args.role.clone(),
        is_core: false,
        worktree: worktree_path.clone(),
        branch: branch.clone(),
        backend,
        baseline: String::new(),
        instructions,
        task,
        skills,
        model: model.clone(),
        role_names_for_preamble: catalog.role_names(),
        created_fresh_worktree: created_fresh,
        repo: repo.clone(),
    };

    let (dispatch, dispatch_dir) = launch::launch_dispatch(
        env,
        &store,
        &log,
        spec,
        dispatch_id.clone(),
        (model.model.clone(), model.reasoning_effort.clone()),
    )?;

    if let Some(record) = &env_binding {
        let mut dispatch_record = store.load_dispatch(&run_id, &dispatch.id)?;
        dispatch_record.env_id = Some(record.env_id.clone());
        dispatch_record.image_id = Some(record.image_id.clone());
        store.save_dispatch(&dispatch_record)?;
    }

    Ok(json!({
        "run_id": run_id,
        "dispatch_id": dispatch.id,
        "role": args.role,
        "worktree": dispatch.worktree,
        "branch": dispatch.branch,
        "dispatch_dir": dispatch_dir.to_string_lossy(),
    }))
}

/// Whether this Dispatch gets an execution environment is two reads of the role definition
/// (ticket Architecture): `exec_environment` decides it, and nothing else does — not the role's
/// name, not what the Run happens to be doing. A role that says `false` never gets one, even
/// when the Run carries a profile; a role that says `true` gets one only when the Run actually
/// has a profile to give it.
fn bring_up_environment(
    env: &dyn Environment,
    log: &crate::event_log::EventLog,
    run_id: &str,
    role_def: &RoleDefinition,
    role: &str,
    worktree: &Path,
    exec: &dyn ExecEnvironments,
) -> Result<Option<crate::env::EnvRecord>> {
    if !role_def.exec_environment {
        return Ok(None);
    }
    let Some(profile) = EnvStore::open(env)?.run_profile(run_id) else {
        return Ok(None);
    };
    let record = exec.ensure(env, log, run_id, &profile, role, worktree)?;
    Ok(Some(record))
}

fn render_execution_environment_block(record: &crate::env::EnvRecord) -> String {
    format!(
        "<execution-environment>\nenv_id: {}\nimage_id: {}\n</execution-environment>",
        record.env_id, record.image_id
    )
}

fn render_prior_verification_block(record: &crate::env::EnvRecord, reusable: &Value) -> String {
    format!(
        "## Prior verification\n\nThe execution ledger's reusable entries for this worktree, judged against \
         environment `{}`:\n\n```json\n{}\n```",
        record.env_id,
        serde_json::to_string_pretty(reusable).unwrap_or_else(|_| "null".to_string())
    )
}
