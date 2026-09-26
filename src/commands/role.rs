use super::generate_id;
use crate::environment::Environment;
use crate::error::{codes, err};
use crate::launch::{self, LaunchSpec};
use crate::role::{Backend, RoleCatalog, StartLocation};
use crate::store::{RunRecord, Store};
use crate::worktree;
use anyhow::Result;
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use std::path::PathBuf;
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

pub fn run(command: RoleCommand, env: &dyn Environment, catalog: &dyn RoleCatalog) -> Result<Value> {
    match command {
        RoleCommand::Fire(args) => fire(args, env, catalog),
    }
}

fn fire(args: FireArgs, env: &dyn Environment, catalog: &dyn RoleCatalog) -> Result<Value> {
    let run_id = env
        .var("OAT_RUN_ID")
        .ok_or_else(|| err(codes::RUN_NOT_BOUND, "role fire runs inside a Run; OAT_RUN_ID is not set"))?;
    let task = crate::resolve_text_input(&args.prompt, &args.input_file)?;

    let store = Store::open(env)?;
    let run_record: RunRecord = store.load_run(&run_id)?;
    let repo = PathBuf::from(&run_record.repo);

    let agent = args.agent.clone().unwrap_or_else(|| run_record.backend.clone());
    let backend = Backend::from_str(&agent)?;

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
    let spec = LaunchSpec {
        run: run_record.clone(),
        role_label: args.role.clone(),
        is_core: false,
        worktree: worktree_path.clone(),
        branch: branch.clone(),
        backend,
        baseline: String::new(),
        instructions: vec![role_def.instructions.clone()],
        task,
        skills: role_def.skills.clone(),
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

    Ok(json!({
        "run_id": run_id,
        "dispatch_id": dispatch.id,
        "role": args.role,
        "worktree": dispatch.worktree,
        "branch": dispatch.branch,
        "dispatch_dir": dispatch_dir.to_string_lossy(),
    }))
}
