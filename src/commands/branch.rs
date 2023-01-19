use std::collections::BTreeSet;

use clap::builder::NonEmptyStringValueParser;
use itertools::Itertools;
use jujutsu_lib::backend::{CommitId, ObjectId};
use jujutsu_lib::op_store::RefTarget;
use jujutsu_lib::repo::RepoRef;
use jujutsu_lib::view::View;

use crate::cli_util::{
    user_error, user_error_with_hint, CommandError, CommandHelper, RevisionArg,
    WorkspaceCommandHelper,
};
use crate::commands::make_branch_term;
use crate::formatter::Formatter;
use crate::ui::Ui;

/// Manage branches.
///
/// For information about branches, see
/// https://github.com/martinvonz/jj/blob/main/docs/branches.md.
#[derive(clap::Subcommand, Clone, Debug)]
pub enum BranchSubcommand {
    /// Create a new branch.
    #[command(visible_alias("c"))]
    Create {
        /// The branch's target revision.
        #[arg(long, short)]
        revision: Option<RevisionArg>,

        /// The branches to create.
        #[arg(required = true, value_parser=NonEmptyStringValueParser::new())]
        names: Vec<String>,
    },

    /// Delete an existing branch and propagate the deletion to remotes on the
    /// next push.
    #[command(visible_alias("d"))]
    Delete {
        /// The branches to delete.
        #[arg(required = true)]
        names: Vec<String>,
    },

    /// Forget everything about a branch, including its local and remote
    /// targets.
    ///
    /// A forgotten branch will not impact remotes on future pushes. It will be
    /// recreated on future pulls if it still exists in the remote.
    #[command(visible_alias("f"))]
    Forget {
        /// The branches to forget.
        #[arg(required_unless_present_any(&["glob"]))]
        names: Vec<String>,

        /// A glob pattern indicating branches to forget.
        #[arg(long)]
        glob: Vec<String>,
    },

    /// List branches and their targets
    ///
    /// A remote branch will be included only if its target is different from
    /// the local target. For a conflicted branch (both local and remote), old
    /// target revisions are preceded by a "-" and new target revisions are
    /// preceded by a "+". For information about branches, see
    /// https://github.com/martinvonz/jj/blob/main/docs/branches.md.
    #[command(visible_alias("l"))]
    List,

    /// Update a given branch to point to a certain commit.
    #[command(visible_alias("s"))]
    Set {
        /// The branch's target revision.
        #[arg(long, short)]
        revision: Option<RevisionArg>,

        /// Allow moving the branch backwards or sideways.
        #[arg(long, short = 'B')]
        allow_backwards: bool,

        /// The branches to update.
        #[arg(required = true)]
        names: Vec<String>,
    },
}

pub fn cmd_branch(
    ui: &mut Ui,
    command: &CommandHelper,
    subcommand: &BranchSubcommand,
) -> Result<(), CommandError> {
    let mut workspace_command = command.workspace_helper(ui)?;
    let view = workspace_command.repo().view();
    fn validate_branch_names_exist<'a>(
        view: &'a View,
        names: &'a [String],
    ) -> Result<(), CommandError> {
        for branch_name in names {
            if view.get_local_branch(branch_name).is_none() {
                return Err(user_error(format!("No such branch: {branch_name}")));
            }
        }
        Ok(())
    }

    fn find_globs(view: &View, globs: &[String]) -> Result<Vec<String>, CommandError> {
        let globs: Vec<glob::Pattern> = globs
            .iter()
            .map(|glob| glob::Pattern::new(glob))
            .try_collect()?;
        let matching_branches = view
            .branches()
            .iter()
            .map(|(branch_name, _branch_target)| branch_name)
            .filter(|branch_name| globs.iter().any(|glob| glob.matches(branch_name)))
            .cloned()
            .collect();
        Ok(matching_branches)
    }

    match subcommand {
        BranchSubcommand::Create { revision, names } => {
            let branch_names: Vec<&str> = names
                .iter()
                .map(|branch_name| match view.get_local_branch(branch_name) {
                    Some(_) => Err(user_error_with_hint(
                        format!("Branch already exists: {branch_name}"),
                        "Use `jj branch set` to update it.",
                    )),
                    None => Ok(branch_name.as_str()),
                })
                .try_collect()?;

            if branch_names.len() > 1 {
                writeln!(
                    ui.warning(),
                    "warning: Creating multiple branches ({}).",
                    branch_names.len()
                )?;
            }

            let target_commit =
                workspace_command.resolve_single_rev(revision.as_deref().unwrap_or("@"))?;
            let mut tx = workspace_command.start_transaction(&format!(
                "create {} pointing to commit {}",
                make_branch_term(&branch_names),
                target_commit.id().hex()
            ));
            for branch_name in branch_names {
                tx.mut_repo().set_local_branch(
                    branch_name.to_string(),
                    RefTarget::Normal(target_commit.id().clone()),
                );
            }
            workspace_command.finish_transaction(ui, tx)?;
        }

        BranchSubcommand::Set {
            revision,
            allow_backwards,
            names: branch_names,
        } => {
            if branch_names.len() > 1 {
                writeln!(
                    ui.warning(),
                    "warning: Updating multiple branches ({}).",
                    branch_names.len()
                )?;
            }

            let target_commit =
                workspace_command.resolve_single_rev(revision.as_deref().unwrap_or("@"))?;
            if !allow_backwards
                && !branch_names.iter().all(|branch_name| {
                    is_fast_forward(
                        workspace_command.repo().as_repo_ref(),
                        branch_name,
                        target_commit.id(),
                    )
                })
            {
                return Err(user_error_with_hint(
                    "Refusing to move branch backwards or sideways.",
                    "Use --allow-backwards to allow it.",
                ));
            }
            let mut tx = workspace_command.start_transaction(&format!(
                "point {} to commit {}",
                make_branch_term(branch_names),
                target_commit.id().hex()
            ));
            for branch_name in branch_names {
                tx.mut_repo().set_local_branch(
                    branch_name.to_string(),
                    RefTarget::Normal(target_commit.id().clone()),
                );
            }
            workspace_command.finish_transaction(ui, tx)?;
        }

        BranchSubcommand::Delete { names } => {
            validate_branch_names_exist(view, names)?;
            let mut tx =
                workspace_command.start_transaction(&format!("delete {}", make_branch_term(names)));
            for branch_name in names {
                tx.mut_repo().remove_local_branch(branch_name);
            }
            workspace_command.finish_transaction(ui, tx)?;
        }

        BranchSubcommand::Forget { names, glob } => {
            validate_branch_names_exist(view, names)?;
            let globbed_names = find_globs(view, glob)?;
            let names: BTreeSet<String> = names.iter().cloned().chain(globbed_names).collect();
            let branch_term = make_branch_term(names.iter().collect_vec().as_slice());
            let mut tx = workspace_command.start_transaction(&format!("forget {branch_term}"));
            for branch_name in names {
                tx.mut_repo().remove_branch(&branch_name);
            }
            workspace_command.finish_transaction(ui, tx)?;
        }

        BranchSubcommand::List => {
            list_branches(ui, command, &workspace_command)?;
        }
    }

    Ok(())
}

fn list_branches(
    ui: &mut Ui,
    _command: &CommandHelper,
    workspace_command: &WorkspaceCommandHelper,
) -> Result<(), CommandError> {
    let repo = workspace_command.repo();

    let print_branch_target =
        |formatter: &mut dyn Formatter, target: Option<&RefTarget>| -> Result<(), CommandError> {
            match target {
                Some(RefTarget::Normal(id)) => {
                    write!(formatter, ": ")?;
                    let commit = repo.store().get_commit(id)?;
                    workspace_command.write_commit_summary(formatter, &commit)?;
                    writeln!(formatter)?;
                }
                Some(RefTarget::Conflict { adds, removes }) => {
                    write!(formatter, " ")?;
                    write!(formatter.labeled("conflict"), "(conflicted)")?;
                    writeln!(formatter, ":")?;
                    for id in removes {
                        let commit = repo.store().get_commit(id)?;
                        write!(formatter, "  - ")?;
                        workspace_command.write_commit_summary(formatter, &commit)?;
                        writeln!(formatter)?;
                    }
                    for id in adds {
                        let commit = repo.store().get_commit(id)?;
                        write!(formatter, "  + ")?;
                        workspace_command.write_commit_summary(formatter, &commit)?;
                        writeln!(formatter)?;
                    }
                }
                None => {
                    writeln!(formatter, " (deleted)")?;
                }
            }
            Ok(())
        };

    let mut formatter = ui.stdout_formatter();
    let formatter = formatter.as_mut();
    let index = repo.index();
    for (name, branch_target) in repo.view().branches() {
        write!(formatter.labeled("branch"), "{name}")?;
        print_branch_target(formatter, branch_target.local_target.as_ref())?;

        for (remote, remote_target) in branch_target
            .remote_targets
            .iter()
            .sorted_by_key(|(name, _target)| name.to_owned())
        {
            if Some(remote_target) == branch_target.local_target.as_ref() {
                continue;
            }
            write!(formatter, "  ")?;
            write!(formatter.labeled("branch"), "@{remote}")?;
            if let Some(local_target) = branch_target.local_target.as_ref() {
                let remote_ahead_count = index
                    .walk_revs(&remote_target.adds(), &local_target.adds())
                    .count();
                let local_ahead_count = index
                    .walk_revs(&local_target.adds(), &remote_target.adds())
                    .count();
                if remote_ahead_count != 0 && local_ahead_count == 0 {
                    write!(formatter, " (ahead by {remote_ahead_count} commits)")?;
                } else if remote_ahead_count == 0 && local_ahead_count != 0 {
                    write!(formatter, " (behind by {local_ahead_count} commits)")?;
                } else if remote_ahead_count != 0 && local_ahead_count != 0 {
                    write!(
                        formatter,
                        " (ahead by {remote_ahead_count} commits, behind by {local_ahead_count} \
                         commits)"
                    )?;
                }
            }
            print_branch_target(formatter, Some(remote_target))?;
        }
    }

    Ok(())
}

fn is_fast_forward(repo: RepoRef, branch_name: &str, new_target_id: &CommitId) -> bool {
    if let Some(current_target) = repo.view().get_local_branch(branch_name) {
        current_target
            .adds()
            .iter()
            .any(|add| repo.index().is_ancestor(add, new_target_id))
    } else {
        true
    }
}