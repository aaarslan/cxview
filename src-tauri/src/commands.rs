use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::comparison;
use crate::db::{Database, FindingQuery};
use crate::error::{AppError, AppResult};
use crate::importer::{self, parse_report};
use crate::models::*;
use crate::patching::{self, JournalFile};
use crate::provider;
use crate::repository;
use crate::validation;

pub struct AppState {
    pub db: Mutex<Database>,
    pub write_lock: Mutex<()>,
    pub storage: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindRepositoryRequest {
    pub report_id: String,
    pub repository_path: String,
    pub scan_prefix: Option<String>,
    pub repository_prefix: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalInput {
    pub diagnosis: String,
    #[serde(default)]
    pub assumptions: Vec<String>,
    pub edits: Vec<TextEdit>,
    #[serde(default)]
    pub behavior_preservation: Vec<String>,
    #[serde(default)]
    pub suggested_tests: Vec<String>,
    #[serde(default)]
    pub unresolved_questions: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest {
    pub task_id: String,
    pub patch_id: String,
    pub reviewed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationRequest {
    pub task_id: String,
    pub candidate_id: String,
    pub approved: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiStateRequest {
    pub profile_id: String,
    pub state: UiState,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBundle {
    pub task: RemediationTask,
    pub findings: Vec<FindingRecord>,
    pub repository: Option<RepositoryContext>,
    pub report: Option<ReportSummary>,
    pub diagnostics: Option<ImportDiagnostics>,
    pub validation_plan: Vec<String>,
    pub privacy_note: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub root: String,
    pub bytes: u64,
    pub retained_objects: Vec<String>,
    pub note: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteWorkspaceRequest {
    pub profile_id: String,
    pub confirmation: String,
}

#[tauri::command]
pub fn get_app_snapshot(state: State<'_, AppState>) -> AppResult<AppSnapshot> {
    // Probed before the database lock is taken: this spawns the provider CLI when it is
    // installed, and every other command waits on the same lock.
    let provider = provider::codex_diagnostic();
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    let profile = db.last_profile()?;
    let report = profile
        .as_ref()
        .and_then(|profile| profile.report_id.as_deref())
        .and_then(|id| db.report_summary(id).ok().flatten())
        .or(db.latest_report()?);
    let report_id = report.as_ref().map(|report| report.id.as_str());
    let diagnostics = report_id.and_then(|id| db.diagnostics_for_report(id).ok().flatten());
    let findings = report_id
        .map(|id| {
            db.findings(&FindingQuery {
                report_id: id.to_owned(),
                ..FindingQuery::default()
            })
            .unwrap_or_default()
        })
        .unwrap_or_default();
    let repository_context = profile.as_ref().and_then(|profile| {
        profile.repository_path.as_deref().and_then(|path| {
            repository::inspect_repository(
                path,
                profile.scan_prefix.as_deref(),
                profile.repository_prefix.as_deref(),
            )
            .ok()
        })
    });
    let tasks = profile
        .as_ref()
        .map(|profile| db.tasks_for_profile(&profile.id).unwrap_or_default())
        .unwrap_or_default();
    Ok(AppSnapshot {
        profile,
        report,
        diagnostics,
        findings,
        repository: repository_context,
        tasks,
        provider,
    })
}

#[tauri::command]
pub fn import_report(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> AppResult<ImportResult> {
    let source = PathBuf::from(&path);
    let metadata = fs::metadata(&source).map_err(|source_error| AppError::Io {
        path: source.clone(),
        source: source_error,
    })?;
    if metadata.len() > importer::MAX_REPORT_BYTES {
        return Err(AppError::OversizedReport(importer::MAX_REPORT_BYTES));
    }
    let bytes = fs::read(&source).map_err(|source_error| AppError::Io {
        path: source.clone(),
        source: source_error,
    })?;
    let parsed = parse_report(
        source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("report.json"),
        &path,
        bytes,
    )?;
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    if let Some(existing) = db.report_by_hash(&parsed.report.sha256)? {
        let diagnostics = db
            .diagnostics_for_report(&existing.id)?
            .unwrap_or(parsed.diagnostics);
        let findings = db.findings(&FindingQuery {
            report_id: existing.id.clone(),
            ..FindingQuery::default()
        })?;
        return Ok(ImportResult {
            report: existing,
            diagnostics,
            findings,
            comparison: None,
        });
    }
    let raw_directory = state.storage.join("reports");
    fs::create_dir_all(&raw_directory).map_err(|source_error| AppError::Io {
        path: raw_directory.clone(),
        source: source_error,
    })?;
    let raw_path = raw_directory.join(format!("{}.json", parsed.report.sha256));
    fs::write(&raw_path, &parsed.raw_bytes).map_err(|source_error| AppError::Io {
        path: raw_path.clone(),
        source: source_error,
    })?;
    let previous_profile = db.last_profile()?;
    let baseline_id = previous_profile
        .as_ref()
        .and_then(|profile| profile.report_id.clone());
    db.insert_report(
        &parsed.report,
        &parsed.diagnostics,
        &raw_path.to_string_lossy(),
        &parsed.findings,
    )?;
    let comparison = if let Some(baseline_id) = baseline_id.as_deref() {
        if baseline_id != parsed.report.id {
            let baseline = db.report_summary(baseline_id)?;
            let baseline_diag = db.diagnostics_for_report(baseline_id)?;
            match (baseline, baseline_diag) {
                (Some(baseline), Some(baseline_diag)) => Some(comparison::compare_reports(
                    &baseline,
                    &baseline_diag,
                    &db.all_findings_for_report(&baseline.id)?,
                    &parsed.report,
                    &parsed.diagnostics,
                    &parsed.findings,
                )),
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };
    if let Some(mut profile) = previous_profile {
        profile.report_id = Some(parsed.report.id.clone());
        db.upsert_profile(&profile, Some(&parsed.report.id))?;
    }
    let findings = db.findings(&FindingQuery {
        report_id: parsed.report.id.clone(),
        ..FindingQuery::default()
    })?;
    let _ = app.emit("cxview://report-imported", &parsed.report.id);
    Ok(ImportResult {
        report: parsed.report,
        diagnostics: parsed.diagnostics,
        findings,
        comparison,
    })
}

#[tauri::command]
pub fn bind_repository(
    state: State<'_, AppState>,
    request: BindRepositoryRequest,
) -> AppResult<RepositoryContext> {
    let context = repository::inspect_repository(
        &request.repository_path,
        request.scan_prefix.as_deref(),
        request.repository_prefix.as_deref(),
    )?;
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    let mut profile = db
        .profile_for_report(&request.report_id)?
        .unwrap_or(Profile {
            id: format!("profile-{}", Uuid::new_v4()),
            name: Path::new(&request.repository_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("repository")
                .to_owned(),
            repository_path: None,
            report_id: None,
            scan_prefix: None,
            repository_prefix: None,
            ui_state: UiState::default(),
        });
    profile.repository_path = Some(context.canonical_path.clone());
    profile.report_id = Some(request.report_id.clone());
    profile.scan_prefix = request.scan_prefix;
    profile.repository_prefix = request.repository_prefix;
    db.upsert_profile(&profile, Some(&request.report_id))?;
    Ok(context)
}

#[tauri::command]
pub fn list_findings(
    state: State<'_, AppState>,
    query: FindingQuery,
) -> AppResult<Vec<FindingSummary>> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    db.findings(&query)
}

#[tauri::command]
pub fn read_raw_locator(
    state: State<'_, AppState>,
    report_id: String,
    locator: String,
) -> AppResult<RawInspection> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    let raw_path = db
        .raw_path(&report_id)?
        .ok_or_else(|| AppError::Message("report snapshot not found".to_owned()))?;
    let raw_path = PathBuf::from(&raw_path);
    let storage_root = fs::canonicalize(&state.storage).map_err(|source| AppError::Io {
        path: state.storage.clone(),
        source,
    })?;
    let canonical_raw = fs::canonicalize(&raw_path).map_err(|source| AppError::Io {
        path: raw_path.clone(),
        source,
    })?;
    if !canonical_raw.starts_with(&storage_root) {
        return Err(AppError::UnsafePath(
            "raw report snapshot is outside CXView app storage".to_owned(),
        ));
    }
    let metadata = fs::metadata(&canonical_raw).map_err(|source| AppError::Io {
        path: canonical_raw.clone(),
        source,
    })?;
    if metadata.len() > importer::MAX_REPORT_BYTES {
        return Err(AppError::OversizedReport(importer::MAX_REPORT_BYTES));
    }
    let bytes = fs::read(&canonical_raw).map_err(|source| AppError::Io {
        path: canonical_raw.clone(),
        source,
    })?;
    let root: Value = serde_json::from_slice(&bytes)?;
    let value = resolve_json_pointer(&root, &locator)?;
    let encoded = serde_json::to_vec(value).map_err(AppError::from)?;
    if encoded.len() > 2 * 1024 * 1024 {
        return Err(AppError::Message(
            "raw locator resolves to more than 2 MiB; inspect the retained report outside the detail view".to_owned(),
        ));
    }
    Ok(RawInspection {
        report_id,
        locator,
        source_path: raw_path.to_string_lossy().into_owned(),
        value: value.clone(),
    })
}

#[tauri::command]
pub fn get_investigation(
    state: State<'_, AppState>,
    finding_id: String,
) -> AppResult<InvestigationBundle> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    let finding = db
        .finding(&finding_id)?
        .ok_or_else(|| AppError::Message("finding not found".to_owned()))?;
    let profile = db.profile_for_report(&finding.summary.report_id)?;
    let context = if let Some(profile) = profile
        .as_ref()
        .filter(|profile| profile.repository_path.is_some())
    {
        let root =
            repository::repository_root(profile.repository_path.as_deref().unwrap_or_default())?;
        repository::load_code_context(
            &root,
            &finding,
            profile.scan_prefix.as_deref(),
            profile.repository_prefix.as_deref(),
        )?
    } else {
        empty_code_context(
            &finding,
            "Bind a repository to enable current-code evidence and source navigation.",
        )
    };
    let playbook = playbook_for(&finding);
    let local_note = db.note(&finding_id)?;
    Ok(InvestigationBundle {
        finding,
        context,
        playbook,
        local_note,
    })
}

#[tauri::command]
pub fn create_task(state: State<'_, AppState>, finding_id: String) -> AppResult<RemediationTask> {
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    let finding = db
        .finding(&finding_id)?
        .ok_or_else(|| AppError::Message("finding not found".to_owned()))?;
    let profile = db
        .profile_for_report(&finding.summary.report_id)?
        .ok_or_else(|| {
            AppError::Message("bind a repository before creating a remediation task".to_owned())
        })?;
    let selected_path = profile.repository_path.as_deref().ok_or_else(|| {
        AppError::Message("bind a repository before creating a remediation task".to_owned())
    })?;
    let root = repository::repository_root(selected_path)?;
    let task_id = format!("task-{}", Uuid::new_v4());
    let snapshot = patching::capture_snapshot(
        &root,
        &task_id,
        &finding.summary.report_id,
        &finding,
        profile.scan_prefix.as_deref(),
        profile.repository_prefix.as_deref(),
    )?;
    let now = chrono::Utc::now().to_rfc3339();
    let task = RemediationTask {
        id: task_id,
        profile_id: profile.id,
        report_id: finding.summary.report_id.clone(),
        finding_ids: vec![finding_id],
        state: TaskState::Investigating,
        created_at: now.clone(),
        updated_at: now,
        snapshot,
        proposal: None,
        diff: None,
        patch_id: None,
        notes: String::new(),
    };
    db.insert_task(&task)?;
    Ok(task)
}

#[tauri::command]
pub fn get_task(state: State<'_, AppState>, task_id: String) -> AppResult<RemediationTask> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    db.task(&task_id)?
        .ok_or_else(|| AppError::Message("remediation task not found".to_owned()))
}

#[tauri::command]
pub fn create_manual_proposal(
    state: State<'_, AppState>,
    task_id: String,
    input: ProposalInput,
) -> AppResult<PatchReview> {
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    let task = db
        .task(&task_id)?
        .ok_or_else(|| AppError::Message("remediation task not found".to_owned()))?;
    let root = repository::repository_root(&task.snapshot.repository_path)?;
    let evidence_refs = if input.evidence_refs.is_empty() {
        task.finding_ids
            .iter()
            .map(|id| format!("finding:{id}"))
            .collect()
    } else {
        input.evidence_refs.clone()
    };
    let proposal = ProposalDocument {
        schema_version: "cxview-proposal-v1".to_owned(),
        task_id: task.id.clone(),
        snapshot_id: task.id.clone(),
        source: ProposalSource::Manual,
        provider: None,
        diagnosis: input.diagnosis,
        assumptions: input.assumptions,
        edits: input.edits,
        behavior_preservation: input.behavior_preservation,
        suggested_tests: input.suggested_tests,
        unresolved_questions: input.unresolved_questions,
        evidence_refs,
    };
    let build = patching::build_patch(&root, &task, &proposal)?;
    db.update_task_proposal(&task.id, &TaskState::ProposalReady, &proposal, &build.diff)?;
    Ok(PatchReview {
        task_id: task.id,
        patch_id: build.patch_id,
        diff: build.diff,
        touched_files: build.touched_files,
        review_ready: true,
        risks: build.risks,
        behavior_rationale: proposal.behavior_preservation,
        files: build.files,
    })
}

#[tauri::command]
pub fn import_proposal(
    state: State<'_, AppState>,
    task_id: String,
    path: String,
) -> AppResult<PatchReview> {
    let proposal_path = PathBuf::from(&path);
    let bytes = fs::read(&proposal_path).map_err(|source| AppError::Io {
        path: proposal_path.clone(),
        source,
    })?;
    let mut proposal: ProposalDocument = serde_json::from_slice(&bytes)
        .map_err(|error| AppError::InvalidJson(format!("proposal JSON is invalid: {error}")))?;
    proposal.source = ProposalSource::Imported;
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    let task = db
        .task(&task_id)?
        .ok_or_else(|| AppError::Message("remediation task not found".to_owned()))?;
    if proposal.task_id != task.id || proposal.snapshot_id != task.id {
        return Err(AppError::Message(
            "imported proposal is bound to a different task or snapshot".to_owned(),
        ));
    }
    let root = repository::repository_root(&task.snapshot.repository_path)?;
    let build = patching::build_patch(&root, &task, &proposal)?;
    db.update_task_proposal(&task.id, &TaskState::ProposalReady, &proposal, &build.diff)?;
    Ok(PatchReview {
        task_id: task.id,
        patch_id: build.patch_id,
        diff: build.diff,
        touched_files: build.touched_files,
        review_ready: true,
        risks: build.risks,
        behavior_rationale: proposal.behavior_preservation,
        files: build.files,
    })
}

#[tauri::command]
pub fn review_patch(state: State<'_, AppState>, task_id: String) -> AppResult<PatchReview> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    let task = db
        .task(&task_id)?
        .ok_or_else(|| AppError::Message("remediation task not found".to_owned()))?;
    let proposal = task.proposal.as_ref().ok_or_else(|| {
        AppError::Message("prepare or import a proposal before reviewing changes".to_owned())
    })?;
    let root = repository::repository_root(&task.snapshot.repository_path)?;
    let build = patching::build_patch(&root, &task, proposal)?;
    Ok(PatchReview {
        task_id,
        patch_id: build.patch_id,
        diff: build.diff,
        touched_files: build.touched_files,
        review_ready: true,
        risks: build.risks,
        behavior_rationale: proposal.behavior_preservation.clone(),
        files: build.files,
    })
}

#[tauri::command]
pub fn apply_reviewed_patch(
    state: State<'_, AppState>,
    request: ApplyRequest,
) -> AppResult<AppliedPatch> {
    if !request.reviewed {
        return Err(AppError::Unsupported(
            "check the reviewed-diff approval before applying a patch".to_owned(),
        ));
    }
    let _guard = state
        .write_lock
        .lock()
        .map_err(|_| AppError::Message("another CXView write is in progress".to_owned()))?;
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    let task = db
        .task(&request.task_id)?
        .ok_or_else(|| AppError::Message("remediation task not found".to_owned()))?;
    let proposal = task
        .proposal
        .as_ref()
        .ok_or_else(|| AppError::Message("no proposal is available for this task".to_owned()))?;
    let root = repository::repository_root(&task.snapshot.repository_path)?;
    let build = patching::build_patch(&root, &task, proposal)?;
    if build.patch_id != request.patch_id {
        return Err(AppError::Message(
            "the displayed patch changed or is stale; review the current diff again".to_owned(),
        ));
    }
    let mut applied =
        patching::apply_checked_patch(&root, &build, &state.storage.join("recovery"))?;
    applied.task_id = task.id.clone();
    let journal = serde_json::to_value(&build.journal).map_err(AppError::from)?;
    db.insert_patch(
        &build.patch_id,
        &task.id,
        &build.diff,
        &build.touched_files,
        &journal,
    )?;
    db.update_task_state(&task.id, &TaskState::Applied, Some(&build.patch_id))?;
    Ok(applied)
}

#[tauri::command]
pub fn undo_cxview_patch(state: State<'_, AppState>, task_id: String) -> AppResult<Vec<String>> {
    let _guard = state
        .write_lock
        .lock()
        .map_err(|_| AppError::Message("another CXView write is in progress".to_owned()))?;
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    let task = db
        .task(&task_id)?
        .ok_or_else(|| AppError::Message("remediation task not found".to_owned()))?;
    let patch_id = task.patch_id.as_deref().ok_or_else(|| {
        AppError::Message("no applied CXView patch is recorded for this task".to_owned())
    })?;
    let (_, journal_value) = db.patch_journal(patch_id)?.ok_or_else(|| {
        AppError::Message(
            "recovery journal is unavailable; inverse change must be reconciled manually"
                .to_owned(),
        )
    })?;
    let journal: Vec<JournalFile> =
        serde_json::from_value(journal_value).map_err(AppError::from)?;
    let root = repository::repository_root(&task.snapshot.repository_path)?;
    let files = patching::undo_patch(&root, &journal)?;
    db.mark_patch_undone(patch_id)?;
    db.update_task_state(&task.id, &TaskState::ProposalReady, None)?;
    db.clear_task_patch(&task.id)?;
    Ok(files)
}

#[tauri::command]
pub fn export_task(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    destination: String,
) -> AppResult<String> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    let task = db
        .task(&task_id)?
        .ok_or_else(|| AppError::Message("remediation task not found".to_owned()))?;
    let findings = task
        .finding_ids
        .iter()
        .filter_map(|id| db.finding(id).ok().flatten())
        .collect::<Vec<_>>();
    let report = db.report_summary(&task.report_id)?;
    let diagnostics = db.diagnostics_for_report(&task.report_id)?;
    let repository_context =
        repository::inspect_repository(&task.snapshot.repository_path, None, None).ok();
    let validation_plan = findings
        .iter()
        .flat_map(|finding| playbook_for(finding).regression_tests)
        .collect::<Vec<_>>();
    let bundle = TaskBundle { task, findings, repository: repository_context, report, diagnostics, validation_plan, privacy_note: "This bundle may contain proprietary source, scanner evidence, paths, and secrets embedded in the selected context. Review before sharing. CXView does not claim secure deletion or encryption.".to_owned() };
    let destination_path = PathBuf::from(destination);
    fs::write(
        &destination_path,
        serde_json::to_vec_pretty(&bundle).map_err(AppError::from)?,
    )
    .map_err(|source| AppError::Io {
        path: destination_path.clone(),
        source,
    })?;
    let _ = app.emit(
        "cxview://task-exported",
        destination_path.to_string_lossy().to_string(),
    );
    Ok(destination_path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn investigation_prompt(state: State<'_, AppState>, finding_id: String) -> AppResult<String> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    let finding = db
        .finding(&finding_id)?
        .ok_or_else(|| AppError::Message("finding not found".to_owned()))?;
    let profile = db.profile_for_report(&finding.summary.report_id)?;
    let context = if let Some(profile) = profile
        .as_ref()
        .filter(|profile| profile.repository_path.is_some())
    {
        let root =
            repository::repository_root(profile.repository_path.as_deref().unwrap_or_default())?;
        repository::load_code_context(
            &root,
            &finding,
            profile.scan_prefix.as_deref(),
            profile.repository_prefix.as_deref(),
        )?
    } else {
        empty_code_context(&finding, "No repository is bound.")
    };
    let playbook = playbook_for(&finding);
    Ok(format!(
        "CXView remediation task\n\nEvidence labels: scanner evidence is reported by Checkmarx; observed locally is read from the selected repository; user supplied and AI hypothesis must remain separate.\n\nFinding: {}\nEngine/rule: {}/{}\nSeverity: {} (original: {})\nReport locator: {}\nFile/package: {}\n\nReported description:\n{}\n\nCurrent-code context:\n{}\n\nUnknowns:\n{}\n\nInvestigation questions:\n{}\n\nOutput contract: return cxview-proposal-v1 with task_id, snapshot_id, evidence_refs, diagnosis, assumptions, exact old_text/new_text edits, behavior_preservation, suggested_tests, unresolved_questions. Never claim tests passed. Do not modify files.\n",
        finding.summary.title,
        finding.summary.engine,
        finding.summary.rule.as_deref().unwrap_or("unknown"),
        finding.summary.severity,
        finding
            .summary
            .original_severity
            .as_deref()
            .unwrap_or("unknown"),
        finding.summary.raw_locator,
        finding
            .summary
            .file_path
            .as_deref()
            .or(finding.summary.package_name.as_deref())
            .unwrap_or("unknown"),
        finding.description.as_deref().unwrap_or("not supplied"),
        context
            .current_source
            .as_deref()
            .or(context
                .sca
                .as_ref()
                .map(|sca| sca.notes.join("; "))
                .as_deref())
            .unwrap_or("no current source context"),
        context.unknowns.join("; "),
        playbook.investigation_questions.join("; ")
    ))
}

#[tauri::command]
pub fn discover_validation(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<Vec<ValidationCandidate>> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    let task = db
        .task(&task_id)?
        .ok_or_else(|| AppError::Message("remediation task not found".to_owned()))?;
    validation::discover_checks(&PathBuf::from(&task.snapshot.repository_path))
}

#[tauri::command]
pub fn run_validation(
    state: State<'_, AppState>,
    request: ValidationRequest,
) -> AppResult<ValidationRun> {
    let _guard = state
        .write_lock
        .lock()
        .map_err(|_| AppError::Message("another repository operation is in progress".to_owned()))?;
    // The task is read and the database lock is released before the check runs. A check may take
    // the full timeout, and holding the lock for that long would stall every other command.
    let task = {
        let db = state
            .db
            .lock()
            .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
        db.task(&request.task_id)?
            .ok_or_else(|| AppError::Message("remediation task not found".to_owned()))?
    };
    let root = repository::repository_root(&task.snapshot.repository_path)?;
    let run = validation::run_check(&root, &task, &request.candidate_id, request.approved)?;
    let next_state = match run.status {
        ValidationStatus::Passed if task.proposal.is_some() && task.patch_id.is_some() => {
            TaskState::AwaitingRescan
        }
        ValidationStatus::Passed => task.state.clone(),
        ValidationStatus::Failed | ValidationStatus::TimedOut | ValidationStatus::Canceled => {
            TaskState::Failed
        }
        _ => task.state.clone(),
    };
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    db.insert_validation(&run)?;
    db.update_task_state(&task.id, &next_state, None)?;
    Ok(run)
}

#[tauri::command]
pub fn validation_history(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<Vec<ValidationRun>> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    db.validations_for_task(&task_id)
}

#[tauri::command]
pub fn save_finding_note(
    state: State<'_, AppState>,
    finding_id: String,
    note: String,
) -> AppResult<()> {
    if note.len() > 32 * 1024 {
        return Err(AppError::OversizedReport(32 * 1024));
    }
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    db.save_note(&finding_id, &note)
}

#[tauri::command]
pub fn save_ui_state(state: State<'_, AppState>, request: UiStateRequest) -> AppResult<()> {
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    db.save_ui_state(&request.profile_id, &request.state)
}

#[tauri::command]
pub fn provider_diagnostic() -> ProviderDiagnostic {
    provider::codex_diagnostic()
}

#[tauri::command]
pub fn run_codex_proposal(state: State<'_, AppState>, task_id: String) -> AppResult<PatchReview> {
    // The provider CLI is user initiated and can take a while, so the database lock is released
    // while it runs and retaken only to persist the returned proposal.
    let task = {
        let db = state
            .db
            .lock()
            .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
        db.task(&task_id)?
            .ok_or_else(|| AppError::Message("remediation task not found".to_owned()))?
    };
    let root = repository::repository_root(&task.snapshot.repository_path)?;
    let (proposal, build, _log) = provider::codex_proposal(&task, &root, &state.storage)?;
    let mut db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    db.update_task_proposal(&task.id, &TaskState::ProposalReady, &proposal, &build.diff)?;
    Ok(PatchReview {
        task_id: task.id,
        patch_id: build.patch_id,
        diff: build.diff,
        touched_files: build.touched_files,
        review_ready: true,
        risks: build.risks,
        behavior_rationale: proposal.behavior_preservation,
        files: build.files,
    })
}

#[tauri::command]
pub fn compare_report_pair(
    state: State<'_, AppState>,
    baseline_report_id: String,
    compared_report_id: String,
) -> AppResult<ComparisonSummary> {
    let db = state
        .db
        .lock()
        .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
    let baseline = db
        .report_summary(&baseline_report_id)?
        .ok_or_else(|| AppError::Message("baseline report not found".to_owned()))?;
    let compared = db
        .report_summary(&compared_report_id)?
        .ok_or_else(|| AppError::Message("compared report not found".to_owned()))?;
    let baseline_diag = db
        .diagnostics_for_report(&baseline_report_id)?
        .ok_or_else(|| AppError::Message("baseline diagnostics not found".to_owned()))?;
    let compared_diag = db
        .diagnostics_for_report(&compared_report_id)?
        .ok_or_else(|| AppError::Message("compared diagnostics not found".to_owned()))?;
    Ok(comparison::compare_reports(
        &baseline,
        &baseline_diag,
        &db.all_findings_for_report(&baseline_report_id)?,
        &compared,
        &compared_diag,
        &db.all_findings_for_report(&compared_report_id)?,
    ))
}

#[tauri::command]
pub fn storage_info(state: State<'_, AppState>) -> AppResult<StorageInfo> {
    storage_info_for_path(&state.storage)
}

fn storage_info_for_path(storage: &Path) -> AppResult<StorageInfo> {
    let mut bytes = 0u64;
    let mut objects = Vec::new();
    if storage.exists() {
        for entry in walkdir::WalkDir::new(storage)
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.file_type().is_file() {
                bytes = bytes
                    .saturating_add(entry.metadata().map(|metadata| metadata.len()).unwrap_or(0));
                objects.push(entry.path().to_string_lossy().into_owned());
            }
        }
    }
    Ok(StorageInfo { root: storage.to_string_lossy().into_owned(), bytes, retained_objects: objects, note: "Reports, snapshots, proposal diffs, and recovery journals are retained in app data. Deletion is not forensic secure deletion.".to_owned() })
}

#[tauri::command]
pub fn delete_workspace_data(
    state: State<'_, AppState>,
    request: DeleteWorkspaceRequest,
) -> AppResult<StorageInfo> {
    if request.confirmation != format!("DELETE {}", request.profile_id) {
        return Err(AppError::Message(
            "type the exact workspace confirmation before deletion".to_owned(),
        ));
    }
    let _guard = state
        .write_lock
        .lock()
        .map_err(|_| AppError::Message("another CXView write is in progress".to_owned()))?;
    let deleted = {
        let mut db = state
            .db
            .lock()
            .map_err(|_| AppError::Database("database lock poisoned".to_owned()))?;
        db.delete_profile_data(&request.profile_id)?
    };
    for raw_path in deleted.raw_paths {
        remove_app_object(&state.storage, Path::new(&raw_path))?;
    }
    for patch_id in deleted.patch_ids {
        for extension in ["patch", "json"] {
            let path = state
                .storage
                .join("recovery")
                .join(format!("{patch_id}.{extension}"));
            if path.exists() {
                remove_app_object(&state.storage, &path)?;
            }
        }
    }
    storage_info_for_path(&state.storage)
}

fn remove_app_object(storage: &Path, path: &Path) -> AppResult<()> {
    let canonical_storage = storage
        .canonicalize()
        .unwrap_or_else(|_| storage.to_owned());
    let canonical_parent = path
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
        .ok_or_else(|| AppError::UnsafePath(path.to_string_lossy().into_owned()))?;
    if !canonical_parent.starts_with(&canonical_storage) {
        return Err(AppError::UnsafePath(
            "refusing to delete an object outside CXView app storage".to_owned(),
        ));
    }
    fs::remove_file(path).map_err(|source| AppError::Io {
        path: path.to_owned(),
        source,
    })?;
    Ok(())
}

fn empty_code_context(finding: &FindingRecord, reason: &str) -> CodeContext {
    CodeContext {
        finding_id: finding.summary.id.clone(),
        match_state: MatchState::Unavailable,
        resolved_path: None,
        relative_path: None,
        current_source: None,
        current_range_start: None,
        current_range_end: None,
        reported_snippet: finding.reported_snippet.clone(),
        reported_nodes: finding.nodes.clone(),
        observed_local: vec![EvidenceStatement {
            label: EvidenceLabel::ObservedLocally,
            text: reason.to_owned(),
            locator: None,
        }],
        unknowns: vec!["No current repository snapshot is available.".to_owned()],
        syntax: None,
        related_files: Vec::new(),
        source_hash: None,
        sca: None,
    }
}

fn resolve_json_pointer<'a>(root: &'a Value, locator: &str) -> AppResult<&'a Value> {
    if locator.is_empty()
        || (locator == "/"
            && root
                .as_object()
                .is_some_and(|object| !object.contains_key("")))
    {
        return Ok(root);
    }
    if !locator.starts_with('/') {
        return Err(AppError::Message(format!(
            "raw locator is not a JSON Pointer: {locator}"
        )));
    }
    let mut current = root;
    for token in locator[1..].split('/') {
        let token = token.replace("~1", "/").replace("~0", "~");
        current = match current {
            Value::Object(object) => object.get(&token),
            Value::Array(items) => token
                .parse::<usize>()
                .ok()
                .and_then(|index| items.get(index)),
            _ => None,
        }
        .ok_or_else(|| AppError::Message(format!("raw locator did not resolve: {locator}")))?;
    }
    Ok(current)
}

fn playbook_for(finding: &FindingRecord) -> RemediationPlaybook {
    let key = format!(
        "{} {} {}",
        finding.summary.title,
        finding.summary.rule.clone().unwrap_or_default(),
        finding.cwe.clone().unwrap_or_default()
    )
    .to_ascii_lowercase();
    if finding.summary.category == FindingCategory::Sca {
        return RemediationPlaybook { id: "sca-node-dependency-v1".to_owned(), title: "Node dependency ownership and upgrade review".to_owned(), last_reviewed: "2026-09-16".to_owned(), applicability: vec!["Node.js package findings with package identity or advisory evidence.".to_owned()], investigation_questions: vec!["Why is this package installed: direct manifest entry or transitive owner?".to_owned(), "Which installed versions and dependency paths are affected?".to_owned(), "What fixed range is actually supplied by the report or a separately approved advisory lookup?".to_owned()], preferred_repairs: vec!["Prefer a compatible update to the responsible direct dependency; let the detected package manager regenerate the lockfile in a disposable staging workspace.".to_owned(), "Preview peer, engine, workspace, integrity, and network/install effects before approval.".to_owned()], behavior_preservation: vec!["Preserve runtime and build behavior, package-manager constraints, and all required workspace manifests.".to_owned()], regression_tests: vec!["Run the repository's actual targeted test/typecheck scripts after reviewing manifest and lockfile diffs.".to_owned()], contraindications: vec!["Do not invent a fixed version, hand-edit integrity hashes, discard dev findings, or run force-upgrade/fix automatically.".to_owned()], references: vec![ReferenceLink { label: "npm explain".to_owned(), url: "https://docs.npmjs.com/cli/v11/commands/npm-explain/".to_owned() }], note: "This playbook is guidance, not an executable repair or a vulnerability scanner.".to_owned() };
    }
    if finding.summary.category == FindingCategory::Iac {
        return RemediationPlaybook { id: "iac-evidence-v1".to_owned(), title: "IaC resource evidence review".to_owned(), last_reviewed: "2026-09-16".to_owned(), applicability: vec!["Imported IaC findings with resource/rule evidence.".to_owned()], investigation_questions: vec!["Is the resource and provider context current?".to_owned(), "What expected versus actual value was supplied by the scanner?".to_owned(), "Which configuration/test change preserves intended infrastructure behavior?".to_owned()], preferred_repairs: vec!["Review the source configuration and provider documentation; export a remediation task for deployment tooling.".to_owned()], behavior_preservation: vec!["Do not deploy or mutate infrastructure from CXView.".to_owned()], regression_tests: vec!["Use repository-local configuration validation if an actual script exists.".to_owned()], contraindications: vec!["Do not claim deployment success or infer provider behavior from a missing field.".to_owned()], references: vec![ReferenceLink { label: "CXView scope".to_owned(), url: "https://docs.checkmarx.com/en/34965-182434-checkmarx-one-reporting.html".to_owned() }], note: "IaC import and source guidance are supported; deployment automation is explicitly out of scope.".to_owned() };
    }
    if key.contains("dangerouslysetinnerhtml")
        || key.contains("xss")
        || key.contains("cross-site")
        || finding.cwe.as_deref().is_some_and(|cwe| cwe.contains("79"))
    {
        return RemediationPlaybook { id: "react-dom-xss-v1".to_owned(), title: "React/DOM XSS contextual review".to_owned(), last_reviewed: "2026-09-16".to_owned(), applicability: vec!["React/DOM findings involving raw HTML, DOM sinks, or context-sensitive untrusted data.".to_owned()], investigation_questions: vec!["Does this feature require rich HTML, or is text-only rendering sufficient?".to_owned(), "What is the actual source and context of the value, and is it trusted by a documented boundary?".to_owned(), "If rich HTML is required, which context-appropriate policy/sanitizer and allowed elements are part of the product contract?".to_owned()], preferred_repairs: vec!["For text-only behavior, render as ordinary JSX text and preserve the visible content contract.".to_owned(), "For intentional rich text, use a narrowly scoped, context-appropriate sanitization policy and regression tests; do not assume a universal sanitizer.".to_owned(), "Validate URL, attribute, script, and HTML contexts separately where applicable.".to_owned()], behavior_preservation: vec!["Preserve legitimate rendering, formatting, links, and loading/error behavior established by the feature.".to_owned()], regression_tests: vec!["Add a negative case for the reported payload and a legitimate rich/text rendering case.".to_owned()], contraindications: vec!["Do not cosmetically rename variables, hide the sink behind a wrapper, add broad casts, suppress the rule, or swallow errors.".to_owned()], references: vec![ReferenceLink { label: "React raw HTML".to_owned(), url: "https://react.dev/reference/react-dom/components/common#dangerously-setting-the-inner-html".to_owned() }, ReferenceLink { label: "OWASP XSS prevention".to_owned(), url: "https://cheatsheetseries.owasp.org/cheatsheets/Cross_Site_Scripting_Prevention_Cheat_Sheet.html".to_owned() }], note: "The playbook asks questions from this finding's code context; it is not an executable repair.".to_owned() };
    }
    let (id, title, approach) = if key.contains("redirect") {
        (
            "unsafe-redirect-v1",
            "Unsafe redirect contextual review",
            "Validate destination against the application's allowed-origin/route contract and preserve legitimate redirect behavior.",
        )
    } else if key.contains("sql") {
        (
            "sql-injection-v1",
            "SQL injection query-boundary review",
            "Use the repository's established parameterized query API and preserve query semantics; do not concatenate untrusted values.",
        )
    } else if key.contains("path") {
        (
            "path-traversal-v1",
            "Path traversal filesystem-boundary review",
            "Resolve and validate paths against the intended root with the repository's platform-aware policy; preserve legitimate file selection.",
        )
    } else if key.contains("command") {
        (
            "command-injection-v1",
            "Command injection process-boundary review",
            "Use structured process arguments and a fixed executable allowlist; preserve the supported operation without shell concatenation.",
        )
    } else if key.contains("ssrf") {
        (
            "ssrf-v1",
            "SSRF network-boundary review",
            "Validate destinations and network policy at the actual request boundary; preserve allowed service access.",
        )
    } else if key.contains("secret") || key.contains("credential") {
        (
            "hardcoded-secret-v1",
            "Hardcoded secret response",
            "Remove the literal from source only as part of a rotation/revocation and approved secret-store migration plan.",
        )
    } else {
        (
            "sast-contextual-review-v1",
            "SAST contextual review",
            "Trace the supplied evidence through the implicated local code and preserve the documented behavior contract.",
        )
    };
    RemediationPlaybook { id: id.to_owned(), title: title.to_owned(), last_reviewed: "2026-09-16".to_owned(), applicability: vec![format!("Applies when the supplied rule/title matches this {} context; verify against current code.", finding.summary.category.category_name_for_ui())], investigation_questions: vec!["Which scanner evidence is actually supplied, and which flow steps are missing?".to_owned(), "What does the current code show at the exact path/range, and what local callers/configuration matter?".to_owned(), "What user-visible, authentication, tenant, API, or package behavior must remain unchanged?".to_owned()], preferred_repairs: vec![approach.to_owned()], behavior_preservation: vec!["Document legitimate inputs and the boundary that must remain supported.".to_owned()], regression_tests: vec!["Add a negative/security case for the reported input and a legitimate-input behavior case.".to_owned()], contraindications: vec!["Do not suppress the rule, rename variables, hide the sink behind a wrapper, weaken authentication, add broad casts, or swallow errors.".to_owned()], references: vec![ReferenceLink { label: "Checkmarx reporting reference".to_owned(), url: "https://docs.checkmarx.com/en/34965-182434-checkmarx-one-reporting.html".to_owned() }], note: "This offline playbook is versioned guidance, not a claimed executable repair.".to_owned() }
}

trait CategoryUiName {
    fn category_name_for_ui(&self) -> &'static str;
}

impl CategoryUiName for FindingCategory {
    fn category_name_for_ui(&self) -> &'static str {
        match self {
            FindingCategory::Sast => "SAST",
            FindingCategory::Sca => "SCA",
            FindingCategory::Iac => "IaC",
            FindingCategory::Unknown => "unknown-engine",
        }
    }
}
