use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use similar::TextDiff;

use crate::error::{AppError, AppResult};
use crate::importer::sha256_hex;
use crate::models::*;
use crate::repository::{resolve_finding_path, safe_relative_path};

const MAX_EDIT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalFile {
    pub path: String,
    pub original_content: Option<String>,
    pub original_hash: String,
    pub post_hash: String,
    pub expected_absent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchBuild {
    pub patch_id: String,
    pub diff: String,
    pub touched_files: Vec<String>,
    pub journal: Vec<JournalFile>,
    pub risks: Vec<String>,
    pub files: Vec<DiffFile>,
}

pub fn capture_snapshot(
    root: &Path,
    task_id: &str,
    report_id: &str,
    finding: &FindingRecord,
    scan_prefix: Option<&str>,
    repository_prefix: Option<&str>,
) -> AppResult<SnapshotManifest> {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_owned());
    let mut targets = Vec::new();
    if let Some(path) = finding.summary.file_path.as_deref() {
        let resolved = resolve_finding_path(
            root,
            Some(path),
            scan_prefix,
            repository_prefix,
            finding.reported_snippet.as_deref(),
        )?;
        if matches!(resolved.state, MatchState::RelocatedWithEvidence) && !resolved.snippet_evidence
        {
            // A filename-only relocation is shown as evidence during investigation, but it does
            // not authorize a snapshot or an edit. See `resolve_finding_path`.
            return Err(AppError::Message(
                "Cannot create an editable task: the reported path was relocated by a unique filename without matching snippet evidence. Bind the correct repository, or map the path explicitly.".to_owned(),
            ));
        }
        if let Some(absolute) = resolved.absolute {
            if let Some(relative) = absolute
                .strip_prefix(&canonical_root)
                .ok()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
            {
                targets.push((relative, absolute, false));
            }
        } else {
            return Err(AppError::Message("Cannot create an editable task because the reported file is unavailable or ambiguous. Investigate or map the path first.".to_owned()));
        }
    } else if finding.summary.category == FindingCategory::Sca {
        let package_json = root.join("package.json");
        if package_json.is_file() {
            targets.push(("package.json".to_owned(), package_json, false));
        }
        let (_, support) = crate::repository::detect_package_manager(Some(root));
        if let Some(lockfile) = support.lockfile {
            let path = root.join(&lockfile);
            if path.is_file() {
                targets.push((lockfile, path, false));
            }
        }
        if targets.is_empty() {
            return Err(AppError::Message(
                "No package manifest or supported lockfile is available for this dependency task."
                    .to_owned(),
            ));
        }
    } else {
        return Err(AppError::Message(
            "This finding has no current file or package identity to snapshot.".to_owned(),
        ));
    }
    let mut files = Vec::new();
    for (relative, path, expected_absent) in targets {
        let bytes = fs::read(&path).map_err(|source| AppError::Io {
            path: path.clone(),
            source,
        })?;
        if bytes.len() > MAX_EDIT_BYTES {
            return Err(AppError::OversizedReport(MAX_EDIT_BYTES as u64));
        }
        let content = String::from_utf8(bytes.clone()).map_err(|_| {
            AppError::Unsupported(format!(
                "{relative} is binary; CXView proposal edits are text-only"
            ))
        })?;
        files.push(FileManifest {
            path: relative,
            sha256: sha256_hex(&bytes),
            byte_length: bytes.len(),
            line_ending: line_ending(&content),
            content: Some(content),
            expected_absent,
        });
    }
    let branch = git_value(root, &["branch", "--show-current"]);
    let head_commit = git_value(root, &["rev-parse", "HEAD"]);
    Ok(SnapshotManifest {
        task_id: task_id.to_owned(),
        report_id: report_id.to_owned(),
        repository_path: root.to_string_lossy().into_owned(),
        branch,
        head_commit,
        captured_at: chrono::Utc::now().to_rfc3339(),
        files,
    })
}

pub fn build_patch(
    root: &Path,
    task: &RemediationTask,
    proposal: &ProposalDocument,
) -> AppResult<PatchBuild> {
    if proposal.schema_version != "cxview-proposal-v1" {
        return Err(AppError::Unsupported(
            "proposal schema version is not supported".to_owned(),
        ));
    }
    if proposal.task_id != task.id || proposal.snapshot_id != task.id {
        return Err(AppError::Message(
            "proposal task/snapshot identity does not match the active task".to_owned(),
        ));
    }
    if proposal.edits.is_empty() {
        return Err(AppError::Message("proposal has no text edits".to_owned()));
    }
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_owned());
    let manifest: BTreeMap<String, &FileManifest> = task
        .snapshot
        .files
        .iter()
        .map(|file| (file.path.clone(), file))
        .collect();
    let mut by_path: BTreeMap<String, Vec<&TextEdit>> = BTreeMap::new();
    for edit in &proposal.edits {
        if edit.path.trim().is_empty() || edit.path.contains('\0') {
            return Err(AppError::UnsafePath(edit.path.clone()));
        }
        let path = safe_relative_path(root, &edit.path, edit.expected_absent)?;
        let normalized = path
            .strip_prefix(&canonical_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if normalized != edit.path.replace('\\', "/") {
            return Err(AppError::OutsideRepository(edit.path.clone()));
        }
        if edit.old_text.len() + edit.new_text.len() > MAX_EDIT_BYTES {
            return Err(AppError::OversizedReport(MAX_EDIT_BYTES as u64));
        }
        if edit.path == ".git" || edit.path.starts_with(".git/") {
            return Err(AppError::UnsafePath(edit.path.clone()));
        }
        if !manifest.contains_key(&edit.path.replace('\\', "/")) && !edit.expected_absent {
            return Err(AppError::Message(format!(
                "proposal target {} was not captured in the task snapshot",
                edit.path
            )));
        }
        by_path
            .entry(edit.path.replace('\\', "/"))
            .or_default()
            .push(edit);
    }
    let mut patch_sections = Vec::new();
    let mut journal = Vec::new();
    let mut files = Vec::new();
    let mut risks = proposal.unresolved_questions.clone();
    for (relative, edits) in by_path {
        let absolute = safe_relative_path(
            root,
            &relative,
            edits.iter().any(|edit| edit.expected_absent),
        )?;
        let current_bytes = fs::read(&absolute).ok();
        let current_content = current_bytes
            .as_deref()
            .map(|bytes| String::from_utf8(bytes.to_vec()))
            .transpose()
            .map_err(|_| {
                AppError::Unsupported(format!("{relative} is binary; binary edits are rejected"))
            })?;
        let snapshot = manifest.get(&relative).copied();
        if edits.iter().any(|edit| edit.expected_absent) {
            if current_bytes.is_some() {
                return Err(AppError::Message(format!(
                    "new-file precondition failed: {relative} already exists"
                )));
            }
            if edits.len() != 1 || !edits[0].old_text.is_empty() {
                return Err(AppError::Unsupported(format!(
                    "new-file proposal for {relative} must contain exactly one empty old_text edit"
                )));
            }
            let new_content = edits[0].new_text.clone();
            let post_hash = sha256_hex(new_content.as_bytes());
            patch_sections.push(new_file_section(&relative, &new_content));
            files.push(DiffFile {
                path: relative.clone(),
                before: String::new(),
                after: new_content.clone(),
            });
            journal.push(JournalFile {
                path: relative,
                original_content: None,
                original_hash: sha256_hex(b""),
                post_hash,
                expected_absent: true,
            });
            continue;
        }
        let current = current_content.ok_or_else(|| AppError::MissingPath(relative.clone()))?;
        let snapshot = snapshot
            .ok_or_else(|| AppError::Message(format!("missing captured base for {relative}")))?;
        if sha256_hex(current.as_bytes()) != snapshot.sha256 {
            return Err(AppError::Message(format!(
                "stale proposal: current bytes for {relative} differ from the captured base hash"
            )));
        }
        let mut ranges = Vec::new();
        let mut next_content = current.clone();
        for edit in &edits {
            let mut positions = current
                .match_indices(&edit.old_text)
                .map(|(offset, _)| offset)
                .collect::<Vec<_>>();
            if edit.old_text.is_empty() {
                positions = vec![0];
            }
            if positions.len() != 1 {
                return Err(AppError::Message(format!(
                    "proposal anchor for {relative} is {} instead of uniquely matched",
                    if positions.is_empty() {
                        "missing"
                    } else {
                        "ambiguous"
                    }
                )));
            }
            let start = positions[0];
            let end = start + edit.old_text.len();
            if ranges
                .iter()
                .any(|(other_start, other_end): &(usize, usize)| {
                    start < *other_end && end > *other_start
                })
            {
                return Err(AppError::Message(format!(
                    "overlapping edits for {relative} are rejected"
                )));
            }
            ranges.push((start, end));
        }
        let mut ordered = edits.clone();
        ordered.sort_by_key(|edit| current.find(&edit.old_text).unwrap_or(usize::MAX));
        for edit in ordered.iter().rev() {
            let start = current
                .find(&edit.old_text)
                .ok_or_else(|| AppError::Message(format!("anchor vanished for {relative}")))?;
            next_content.replace_range(start..start + edit.old_text.len(), &edit.new_text);
        }
        if next_content == current {
            return Err(AppError::Message(format!(
                "proposal for {relative} produces no change"
            )));
        }
        let post_hash = sha256_hex(next_content.as_bytes());
        patch_sections.push(unified_section(&relative, &current, &next_content));
        files.push(DiffFile {
            path: relative.clone(),
            before: current.clone(),
            after: next_content.clone(),
        });
        journal.push(JournalFile {
            path: relative,
            original_content: Some(current),
            original_hash: snapshot.sha256.clone(),
            post_hash,
            expected_absent: false,
        });
    }
    if proposal.behavior_preservation.is_empty() {
        risks.push("Proposal does not state behavior-preservation rationale.".to_owned());
    }
    if proposal.suggested_tests.is_empty() {
        risks.push("No regression-test idea was supplied; passing local checks cannot establish a scanner fix.".to_owned());
    }
    let diff = patch_sections.join("\n");
    let patch_id = format!(
        "patch-{}",
        &sha256_hex(format!("{}|{}", task.id, diff).as_bytes())[..32]
    );
    Ok(PatchBuild {
        patch_id,
        diff,
        touched_files: journal.iter().map(|file| file.path.clone()).collect(),
        journal,
        risks,
        files,
    })
}

pub fn apply_checked_patch(
    root: &Path,
    build: &PatchBuild,
    patch_storage: &Path,
) -> AppResult<AppliedPatch> {
    let (git_root, git_directory) = git_apply_scope(root)?;
    fs::create_dir_all(patch_storage).map_err(|source| AppError::Io {
        path: patch_storage.to_owned(),
        source,
    })?;
    let patch_path = patch_storage.join(format!("{}.patch", build.patch_id));
    let journal_path = patch_storage.join(format!("{}.json", build.patch_id));
    fs::write(&patch_path, &build.diff).map_err(|source| AppError::Io {
        path: patch_path.clone(),
        source,
    })?;
    fs::write(
        &journal_path,
        serde_json::to_vec_pretty(&build.journal).map_err(AppError::from)?,
    )
    .map_err(|source| AppError::Io {
        path: journal_path.clone(),
        source,
    })?;
    for file in &build.journal {
        let path = safe_relative_path(root, &file.path, file.expected_absent)?;
        if file.expected_absent {
            if path.exists() {
                return Err(AppError::Message(format!(
                    "new-file precondition changed before apply: {}",
                    file.path
                )));
            }
        } else {
            let bytes = fs::read(&path).map_err(|source| AppError::Io {
                path: path.clone(),
                source,
            })?;
            if sha256_hex(&bytes) != file.original_hash {
                return Err(AppError::Message(format!(
                    "target changed after review: {}",
                    file.path
                )));
            }
        }
    }
    let check = git_apply_command(&git_root, git_directory.as_deref(), &patch_path, true)
        .output()
        .map_err(|error| AppError::Process(format!("could not run git apply --check: {error}")))?;
    if !check.status.success() {
        return Err(AppError::Process(format!(
            "git apply --check rejected the reviewed patch: {}",
            bounded_output(&check.stderr)
        )));
    }
    let applied = git_apply_command(&git_root, git_directory.as_deref(), &patch_path, false)
        .output()
        .map_err(|error| AppError::Process(format!("could not apply reviewed patch: {error}")))?;
    if !applied.status.success() {
        let changed = build
            .journal
            .iter()
            .filter_map(|file| {
                let path = root.join(&file.path);
                let bytes = fs::read(path).ok()?;
                (sha256_hex(&bytes) != file.original_hash).then_some(file.path.clone())
            })
            .collect::<Vec<_>>();
        let partial = if changed.is_empty() {
            "No target changed.".to_owned()
        } else {
            format!(
                "Potential partial application on: {}. Recovery journal: {}",
                changed.join(", "),
                journal_path.display()
            )
        };
        return Err(AppError::Process(format!(
            "git apply failed: {} {partial}",
            bounded_output(&applied.stderr)
        )));
    }
    let mut post_hashes = Vec::new();
    for file in &build.journal {
        let path = root.join(&file.path);
        let bytes = fs::read(&path).map_err(|source| AppError::Io {
            path: path.clone(),
            source,
        })?;
        let hash = sha256_hex(&bytes);
        if hash != file.post_hash {
            return Err(AppError::Message(format!(
                "post-application hash did not match the reviewed result for {}",
                file.path
            )));
        }
        post_hashes.push(FileHash {
            path: file.path.clone(),
            sha256: hash,
        });
    }
    Ok(AppliedPatch { patch_id: build.patch_id.clone(), task_id: String::new(), applied_at: chrono::Utc::now().to_rfc3339(), touched_files: build.touched_files.clone(), post_hashes, undo_available: true, message: "Reviewed patch applied to the working tree without staging, committing, stashing, or resetting.".to_owned() })
}

fn git_apply_scope(root: &Path) -> AppResult<(PathBuf, Option<String>)> {
    let git_root = git_value(root, &["rev-parse", "--show-toplevel"]).ok_or_else(|| {
        AppError::Unsupported(
            "checked patch application requires a Git repository; export the reviewed patch for an ordinary folder".to_owned(),
        )
    })?;
    let git_root = PathBuf::from(git_root)
        .canonicalize()
        .map_err(|source| AppError::Io {
            path: root.to_owned(),
            source,
        })?;
    let selected_root = root.canonicalize().map_err(|source| AppError::Io {
        path: root.to_owned(),
        source,
    })?;
    let prefix = selected_root
        .strip_prefix(&git_root)
        .map_err(|_| {
            AppError::OutsideRepository(
                "selected repository is not contained by its Git worktree".to_owned(),
            )
        })?
        .to_string_lossy()
        .replace('\\', "/");
    Ok((git_root, (!prefix.is_empty()).then_some(prefix)))
}

fn git_apply_command(
    git_root: &Path,
    directory: Option<&str>,
    patch_path: &Path,
    check: bool,
) -> Command {
    let mut command = Command::new("git");
    command.arg("apply");
    if check {
        command.arg("--check");
    } else {
        command.arg("--whitespace=nowarn");
    }
    if let Some(directory) = directory {
        command.arg(format!("--directory={directory}"));
    }
    command
        .arg(patch_path.to_string_lossy().as_ref())
        .current_dir(git_root);
    command
}

pub fn undo_patch(root: &Path, journal: &[JournalFile]) -> AppResult<Vec<String>> {
    let mut conflicts = Vec::new();
    for file in journal {
        let path = safe_relative_path(root, &file.path, file.expected_absent)?;
        if file.expected_absent {
            let bytes = fs::read(&path).map_err(|_| {
                AppError::Message(format!(
                    "undo conflict: new file {} is missing or unreadable",
                    file.path
                ))
            })?;
            if sha256_hex(&bytes) != file.post_hash {
                conflicts.push(file.path.clone());
            }
        } else {
            let bytes = fs::read(&path).map_err(|source| AppError::Io {
                path: path.clone(),
                source,
            })?;
            if sha256_hex(&bytes) != file.post_hash {
                conflicts.push(file.path.clone());
            }
        }
    }
    if !conflicts.is_empty() {
        return Err(AppError::Message(format!(
            "undo stopped to protect subsequent edits; post-image mismatch in {}. Export the inverse change for manual reconciliation.",
            conflicts.join(", ")
        )));
    }
    for file in journal {
        let path = root.join(&file.path);
        if file.expected_absent {
            fs::remove_file(&path).map_err(|source| AppError::Io {
                path: path.clone(),
                source,
            })?;
        } else if let Some(content) = &file.original_content {
            fs::write(&path, content.as_bytes()).map_err(|source| AppError::Io {
                path: path.clone(),
                source,
            })?;
        }
    }
    Ok(journal.iter().map(|file| file.path.clone()).collect())
}

pub fn snapshot_hash(snapshot: &SnapshotManifest) -> String {
    let value = snapshot
        .files
        .iter()
        .map(|file| format!("{}:{}", file.path, file.sha256))
        .collect::<Vec<_>>()
        .join("|");
    sha256_hex(value.as_bytes())
}

fn unified_section(path: &str, old: &str, new: &str) -> String {
    let diff = TextDiff::from_lines(old, new);
    let body = diff
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string();
    format!("diff --git a/{path} b/{path}\n{body}")
}

fn new_file_section(path: &str, content: &str) -> String {
    let diff = TextDiff::from_lines("", content);
    let body = diff
        .unified_diff()
        .context_radius(3)
        .header("/dev/null", &format!("b/{path}"))
        .to_string();
    format!("diff --git a/{path} b/{path}\nnew file mode 100644\n{body}")
}

fn line_ending(content: &str) -> String {
    if content.contains("\r\n") {
        "CRLF".to_owned()
    } else {
        "LF".to_owned()
    }
}

fn git_value(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn bounded_output(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.chars().take(4000).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn task(root: &Path) -> RemediationTask {
        let task_id = "task-test";
        let original = "export const value = '<unsafe>'\n";
        RemediationTask {
            id: task_id.to_owned(),
            profile_id: "profile".to_owned(),
            report_id: "report".to_owned(),
            finding_ids: vec!["finding".to_owned()],
            state: TaskState::Investigating,
            created_at: "now".to_owned(),
            updated_at: "now".to_owned(),
            snapshot: SnapshotManifest {
                task_id: task_id.to_owned(),
                report_id: "report".to_owned(),
                repository_path: root.to_string_lossy().into_owned(),
                branch: None,
                head_commit: None,
                captured_at: "now".to_owned(),
                files: vec![FileManifest {
                    path: "src/App.tsx".to_owned(),
                    sha256: sha256_hex(original.as_bytes()),
                    byte_length: original.len(),
                    line_ending: "LF".to_owned(),
                    content: Some(original.to_owned()),
                    expected_absent: false,
                }],
            },
            proposal: None,
            diff: None,
            patch_id: None,
            notes: String::new(),
        }
    }

    #[test]
    fn rejects_ambiguous_anchor_and_accepts_unique_edit() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("src")).unwrap();
        fs::write(
            directory.path().join("src/App.tsx"),
            "export const value = '<unsafe>'\n",
        )
        .unwrap();
        let task = task(directory.path());
        let proposal = ProposalDocument {
            schema_version: "cxview-proposal-v1".to_owned(),
            task_id: task.id.clone(),
            snapshot_id: task.id.clone(),
            source: ProposalSource::Manual,
            provider: None,
            diagnosis: "Replace raw output in this text-only fixture.".to_owned(),
            assumptions: vec![],
            edits: vec![TextEdit {
                path: "src/App.tsx".to_owned(),
                old_text: "'<unsafe>'".to_owned(),
                new_text: "'safe'".to_owned(),
                expected_absent: false,
            }],
            behavior_preservation: vec!["Value remains a string.".to_owned()],
            suggested_tests: vec!["npm test".to_owned()],
            unresolved_questions: vec![],
            evidence_refs: vec!["/scanResults/0".to_owned()],
        };
        let patch = build_patch(directory.path(), &task, &proposal).unwrap();
        assert!(patch.diff.contains("-export const value = '<unsafe>'"));
        assert_eq!(patch.journal.len(), 1);
    }

    #[test]
    fn stale_base_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("src")).unwrap();
        fs::write(directory.path().join("src/App.tsx"), "changed\n").unwrap();
        let task = task(directory.path());
        let proposal = ProposalDocument {
            schema_version: "cxview-proposal-v1".to_owned(),
            task_id: task.id.clone(),
            snapshot_id: task.id.clone(),
            source: ProposalSource::Manual,
            provider: None,
            diagnosis: "stale".to_owned(),
            assumptions: vec![],
            edits: vec![TextEdit {
                path: "src/App.tsx".to_owned(),
                old_text: "'<unsafe>'".to_owned(),
                new_text: "'safe'".to_owned(),
                expected_absent: false,
            }],
            behavior_preservation: vec![],
            suggested_tests: vec![],
            unresolved_questions: vec![],
            evidence_refs: vec![],
        };
        assert!(build_patch(directory.path(), &task, &proposal).is_err());
    }

    #[test]
    fn checked_git_application_and_undo_protect_unrelated_and_subsequent_work() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/App.tsx"),
            "export const value = '<unsafe>'\n",
        )
        .unwrap();
        fs::write(root.join("unrelated.txt"), "user staged or unstaged work\n").unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(root)
                .status()
                .unwrap();
            assert!(status.success(), "git command failed: {args:?}");
        };
        git(&["init", "-q"]);
        git(&["add", "."]);
        git(&[
            "-c",
            "user.email=cxview@example.invalid",
            "-c",
            "user.name=CXView Test",
            "commit",
            "-qm",
            "base",
        ]);
        fs::write(root.join("unrelated.txt"), "user changed after commit\n").unwrap();
        let task = task(root);
        let proposal = ProposalDocument {
            schema_version: "cxview-proposal-v1".to_owned(),
            task_id: task.id.clone(),
            snapshot_id: task.id.clone(),
            source: ProposalSource::Manual,
            provider: None,
            diagnosis: "Replace raw output in this text-only fixture.".to_owned(),
            assumptions: vec![],
            edits: vec![TextEdit {
                path: "src/App.tsx".to_owned(),
                old_text: "'<unsafe>'".to_owned(),
                new_text: "'safe'".to_owned(),
                expected_absent: false,
            }],
            behavior_preservation: vec!["Value remains a string.".to_owned()],
            suggested_tests: vec!["node --test".to_owned()],
            unresolved_questions: vec![],
            evidence_refs: vec!["/scanResults/0".to_owned()],
        };
        let build = build_patch(root, &task, &proposal).unwrap();
        apply_checked_patch(root, &build, &root.join("recovery")).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("src/App.tsx")).unwrap(),
            "export const value = 'safe'\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("unrelated.txt")).unwrap(),
            "user changed after commit\n"
        );
        undo_patch(root, &build.journal).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("src/App.tsx")).unwrap(),
            "export const value = '<unsafe>'\n"
        );
        fs::write(root.join("src/App.tsx"), "user follow-up edit\n").unwrap();
        assert!(undo_patch(root, &build.journal).is_err());
    }

    #[test]
    fn checked_application_handles_a_selected_nested_monorepo_root() {
        let directory = tempfile::tempdir().unwrap();
        let git_root = directory.path();
        let selected_root = git_root.join("packages/web");
        fs::create_dir_all(selected_root.join("src")).unwrap();
        fs::write(
            selected_root.join("src/App.tsx"),
            "export const value = '<unsafe>'\n",
        )
        .unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(git_root)
                .status()
                .unwrap();
            assert!(status.success(), "git command failed: {args:?}");
        };
        git(&["init", "-q"]);
        git(&["add", "."]);
        git(&[
            "-c",
            "user.email=cxview@example.invalid",
            "-c",
            "user.name=CXView Test",
            "commit",
            "-qm",
            "base",
        ]);
        let task = task(&selected_root);
        let proposal = ProposalDocument {
            schema_version: "cxview-proposal-v1".to_owned(),
            task_id: task.id.clone(),
            snapshot_id: task.id.clone(),
            source: ProposalSource::Manual,
            provider: None,
            diagnosis: "Replace raw output in the selected package root.".to_owned(),
            assumptions: vec![],
            edits: vec![TextEdit {
                path: "src/App.tsx".to_owned(),
                old_text: "'<unsafe>'".to_owned(),
                new_text: "'safe'".to_owned(),
                expected_absent: false,
            }],
            behavior_preservation: vec!["Value remains a string.".to_owned()],
            suggested_tests: vec!["node --test".to_owned()],
            unresolved_questions: vec![],
            evidence_refs: vec![],
        };
        let build = build_patch(&selected_root, &task, &proposal).unwrap();
        apply_checked_patch(&selected_root, &build, &selected_root.join("recovery")).unwrap();
        assert_eq!(
            fs::read_to_string(selected_root.join("src/App.tsx")).unwrap(),
            "export const value = 'safe'\n"
        );
        undo_patch(&selected_root, &build.journal).unwrap();
        assert_eq!(
            fs::read_to_string(selected_root.join("src/App.tsx")).unwrap(),
            "export const value = '<unsafe>'\n"
        );
    }
}
