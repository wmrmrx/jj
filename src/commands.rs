use std::collections::{BTreeSet, HashSet};
use std::ops::Deref;
use std::sync::Mutex;
use jujutsu_lib::matchers::EverythingMatcher;
use jujutsu_lib::{conflicts, file_util, git, revset};
use crate::diff_util::{self, DiffFormat, DiffFormatArgs};
use crate::formatter::Formatter;
    diff_util::show_diff(
        diff_util::diff_format_for(ui, &args.format),
    diff_util::show_patch(
        &commit,
        &EverythingMatcher,
        diff_util::diff_format_for(ui, &args.format),
            diff_util::show_diff_summary(
    let committer_timestamp = if settings.relative_timestamps() {
        "committer.timestamp().ago()"
        "committer.timestamp()"
            " " label("timestamp", {committer_timestamp})
        .then(|| diff_util::diff_format_for(ui, &args.diff_format));
                    diff_util::show_patch(
                    diff_util::show_patch(
        .then(|| diff_util::diff_format_for(ui, &args.diff_format));
    diff_util::show_diff(formatter, workspace_command, diff_iterator, diff_format)
    diff_util::show_diff(
        diff_util::diff_format_for(ui, &args.format),
    let diff_summary_bytes =
        diff_util::diff_as_bytes(workspace_command, diff_iter, DiffFormat::Summary)?;