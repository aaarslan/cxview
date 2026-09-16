use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::importer::sha256_hex;
use crate::models::*;
use crate::patching::snapshot_hash;
use crate::repository::{detect_package_manager, safe_relative_path};

const MAX_OUTPUT_BYTES: usize = 96 * 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15 * 60);

pub fn discover_checks(root: &Path) -> AppResult<Vec<ValidationCandidate>> {
    let package_root =
        crate::repository::nearest_package_root(root, None).unwrap_or_else(|| root.to_owned());
    let package_json_path = package_root.join("package.json");
    let package_json_bytes = fs::read(&package_json_path).map_err(|source| AppError::Io {
        path: package_json_path.clone(),
        source,
    })?;
    let package_json: Value = serde_json::from_slice(&package_json_bytes)?;
    let scripts = package_json
        .get("scripts")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AppError::Message(
                "package.json has no scripts object; CXView will not invent checks".to_owned(),
            )
        })?;
    let (manager, support) = detect_package_manager(Some(&package_root));
    let manager = manager.unwrap_or_else(|| "npm".to_owned());
    let executable = manager.clone();
    let mut candidates = Vec::new();
    for (name, definition) in scripts {
        if !is_check_name(name) {
            continue;
        }
        let args = match manager.as_str() {
            "npm" => vec!["run".to_owned(), name.clone()],
            "pnpm" => vec!["run".to_owned(), name.clone()],
            "yarn" => vec![name.clone()],
            "bun" => vec!["run".to_owned(), name.clone()],
            _ => vec!["run".to_owned(), name.clone()],
        };
        let source_hash = sha256_hex(definition.as_str().unwrap_or_default().as_bytes());
        candidates.push(ValidationCandidate { id: format!("script-{name}"), label: format!("{name} — {}", definition.as_str().unwrap_or("script")), executable: executable.clone(), args, working_directory: package_root.to_string_lossy().into_owned(), source: "package.json scripts".to_owned(), script_definition_hash: Some(source_hash), expected_writes: vec!["Repository-managed outputs declared by the script may be written.".to_owned()], network_note: "The script and its lifecycle hooks may access the network; CXView does not rewrite or sandbox repository scripts.".to_owned() });
    }
    if candidates.is_empty() {
        return Err(AppError::Message("No test, typecheck, lint, or build-like scripts were found in the actual package.json; no check was invented.".to_owned()));
    }
    if !support.supported_lockfile && support.lockfile.is_some() {
        for candidate in &mut candidates {
            candidate.network_note.push_str(" Lockfile format is not parsed by CXView; package-manager hooks remain the repository's responsibility.");
        }
    }
    Ok(candidates)
}

pub fn run_check(
    root: &Path,
    task: &RemediationTask,
    candidate_id: &str,
    approved: bool,
) -> AppResult<ValidationRun> {
    if !approved {
        return Err(AppError::Unsupported(
            "command approval is required before executing a repository script".to_owned(),
        ));
    }
    let candidates = discover_checks(root)?;
    let candidate = candidates
        .into_iter()
        .find(|candidate| candidate.id == candidate_id)
        .ok_or_else(|| {
            AppError::Message(
                "validation candidate is no longer present in the current package.json".to_owned(),
            )
        })?;
    let package_json = PathBuf::from(&candidate.working_directory).join("package.json");
    let current_script_hash = fs::read_to_string(&package_json)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| {
            value
                .get("scripts")
                .and_then(Value::as_object)
                .and_then(|scripts| {
                    scripts
                        .get(candidate_id.strip_prefix("script-").unwrap_or(candidate_id))
                        .and_then(Value::as_str)
                        .map(|script| sha256_hex(script.as_bytes()))
                })
        });
    if current_script_hash != candidate.script_definition_hash {
        return Err(AppError::Message(
            "validation definition changed since discovery; review the command again".to_owned(),
        ));
    }

    let expected_hash = expected_current_hash(root, task)?;
    let actual_hash = current_source_hash(root, task)?;
    let started_at = chrono::Utc::now().to_rfc3339();
    if expected_hash != actual_hash {
        return Ok(ValidationRun { id: format!("validation-{}", Uuid::new_v4()), task_id: task.id.clone(), candidate_id: candidate.id, status: ValidationStatus::Unknown, exit_code: None, duration_ms: 0, stdout: String::new(), stderr: String::new(), snapshot_hash: actual_hash, started_at, note: "Current source/configuration no longer matches this task's captured baseline or reviewed proposal; prior evidence is invalidated.".to_owned() });
    }

    let executable = which::which(&candidate.executable).map_err(|_| {
        AppError::Process(format!(
            "{} is not installed or is not on PATH",
            candidate.executable
        ))
    })?;
    let start = Instant::now();
    let mut command = Command::new(executable);
    command
        .args(&candidate.args)
        .current_dir(&candidate.working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env_remove("CXVIEW_PROVIDER_TOKEN");
    let mut child = command.spawn().map_err(|error| {
        AppError::Process(format!("could not start {}: {error}", candidate.executable))
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Process("validation stdout pipe unavailable".to_owned()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::Process("validation stderr pipe unavailable".to_owned()))?;
    let out_buffer = Arc::new(Mutex::new(Vec::new()));
    let err_buffer = Arc::new(Mutex::new(Vec::new()));
    let out_handle = spawn_reader(stdout, Arc::clone(&out_buffer));
    let err_handle = spawn_reader(stderr, Arc::clone(&err_buffer));
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| {
            AppError::Process(format!("could not poll validation process: {error}"))
        })? {
            break status;
        }
        if start.elapsed() >= DEFAULT_TIMEOUT {
            timed_out = true;
            terminate_process(&mut child);
            break child.wait().map_err(|error| {
                AppError::Process(format!(
                    "could not reap timed-out validation process: {error}"
                ))
            })?;
        }
        thread::sleep(Duration::from_millis(40));
    };
    let _ = out_handle.join();
    let _ = err_handle.join();
    let stdout = String::from_utf8_lossy(
        &out_buffer
            .lock()
            .map_err(|_| AppError::Process("stdout buffer lock poisoned".to_owned()))?,
    )
    .into_owned();
    let stderr = String::from_utf8_lossy(
        &err_buffer
            .lock()
            .map_err(|_| AppError::Process("stderr buffer lock poisoned".to_owned()))?,
    )
    .into_owned();
    let status_kind = if timed_out {
        ValidationStatus::TimedOut
    } else if status.success() {
        ValidationStatus::Passed
    } else {
        ValidationStatus::Failed
    };
    let note = if timed_out {
        "The check exceeded the bounded 15 minute timeout and was terminated; no pass is recorded."
            .to_owned()
    } else if status.success() {
        "Process exited 0. This is local validation evidence only; it does not mark the scanner observation fixed.".to_owned()
    } else {
        format!(
            "Process exited with {}; this is a local failure, not a scanner conclusion.",
            status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "no code".to_owned())
        )
    };
    Ok(ValidationRun {
        id: format!("validation-{}", Uuid::new_v4()),
        task_id: task.id.clone(),
        candidate_id: candidate.id,
        status: status_kind,
        exit_code: status.code(),
        duration_ms: start.elapsed().as_millis(),
        stdout,
        stderr,
        snapshot_hash: actual_hash,
        started_at,
        note,
    })
}

fn expected_current_hash(_root: &Path, task: &RemediationTask) -> AppResult<String> {
    let Some(proposal) = &task.proposal else {
        return Ok(snapshot_hash(&task.snapshot));
    };
    let mut entries = Vec::new();
    for manifest in &task.snapshot.files {
        let mut content = manifest.content.clone().ok_or_else(|| {
            AppError::Message(format!(
                "snapshot content unavailable for {}",
                manifest.path
            ))
        })?;
        for edit in proposal
            .edits
            .iter()
            .filter(|edit| edit.path.replace('\\', "/") == manifest.path && !edit.expected_absent)
        {
            let positions = content
                .match_indices(&edit.old_text)
                .map(|(position, _)| position)
                .collect::<Vec<_>>();
            if positions.len() != 1 {
                return Err(AppError::Message(format!(
                    "cannot compute reviewed post-image for {}",
                    manifest.path
                )));
            }
            let position = positions[0];
            content.replace_range(position..position + edit.old_text.len(), &edit.new_text);
        }
        entries.push(format!(
            "{}:{}",
            manifest.path,
            sha256_hex(content.as_bytes())
        ));
    }
    for edit in proposal.edits.iter().filter(|edit| edit.expected_absent) {
        entries.push(format!(
            "{}:{}",
            edit.path.replace('\\', "/"),
            sha256_hex(edit.new_text.as_bytes())
        ));
    }
    Ok(sha256_hex(entries.join("|").as_bytes()))
}

fn current_source_hash(root: &Path, task: &RemediationTask) -> AppResult<String> {
    let mut entries = Vec::new();
    for manifest in &task.snapshot.files {
        let path = safe_relative_path(root, &manifest.path, manifest.expected_absent)?;
        let bytes = fs::read(&path).unwrap_or_default();
        entries.push(format!("{}:{}", manifest.path, sha256_hex(&bytes)));
    }
    if let Some(proposal) = &task.proposal {
        for edit in proposal.edits.iter().filter(|edit| edit.expected_absent) {
            let path = root.join(&edit.path);
            if path.exists() {
                let bytes = fs::read(&path).unwrap_or_default();
                entries.push(format!("{}:{}", edit.path, sha256_hex(&bytes)));
            }
        }
    }
    Ok(sha256_hex(entries.join("|").as_bytes()))
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    buffer: Arc<Mutex<Vec<u8>>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(size) => {
                    if let Ok(mut target) = buffer.lock() {
                        let remaining = MAX_OUTPUT_BYTES.saturating_sub(target.len());
                        target.extend_from_slice(&chunk[..size.min(remaining)]);
                        if target.len() >= MAX_OUTPUT_BYTES {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
        }
    })
}

fn terminate_process(child: &mut Child) {
    #[cfg(unix)]
    {
        let _ = child.kill();
    }
    #[cfg(windows)]
    {
        let pid = child.id();
        let _ = Command::new("taskkill")
            .arg("/PID")
            .arg(pid.to_string())
            .args(["/T", "/F"])
            .status();
    }
}

fn is_check_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "test",
        "check",
        "type",
        "typecheck",
        "lint",
        "build",
        "verify",
        "spec",
    ]
    .iter()
    .any(|word| lower == *word || lower.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scripts_are_discovered_without_inventing_missing_commands() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("package.json"),
            r#"{"scripts":{"test":"node --test","format":"prettier ."}}"#,
        )
        .unwrap();
        let candidates = discover_checks(directory.path()).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].args, vec!["run", "test"]);
    }

    #[test]
    fn actual_manifest_script_runner_records_a_real_exit_code() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let package_json =
            r#"{"scripts":{"test":"node -e \"process.stdout.write('fixture-pass')\""}}"#;
        fs::write(root.join("package.json"), package_json).unwrap();
        let content = package_json.to_owned();
        let task = RemediationTask {
            id: "task-validation".to_owned(),
            profile_id: "profile".to_owned(),
            report_id: "report".to_owned(),
            finding_ids: vec!["finding".to_owned()],
            state: TaskState::Investigating,
            created_at: "now".to_owned(),
            updated_at: "now".to_owned(),
            snapshot: SnapshotManifest {
                task_id: "task-validation".to_owned(),
                report_id: "report".to_owned(),
                repository_path: root.to_string_lossy().into_owned(),
                branch: None,
                head_commit: None,
                captured_at: "now".to_owned(),
                files: vec![FileManifest {
                    path: "package.json".to_owned(),
                    sha256: sha256_hex(content.as_bytes()),
                    byte_length: content.len(),
                    line_ending: "LF".to_owned(),
                    content: Some(content),
                    expected_absent: false,
                }],
            },
            proposal: None,
            diff: None,
            patch_id: None,
            notes: String::new(),
        };
        let candidate = discover_checks(root).unwrap().remove(0);
        let run = run_check(root, &task, &candidate.id, true).unwrap();
        assert!(matches!(run.status, ValidationStatus::Passed));
        assert_eq!(run.exit_code, Some(0));
        assert!(run.stdout.contains("fixture-pass"));
    }
}
