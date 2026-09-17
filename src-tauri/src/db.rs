use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::models::*;

pub struct Database {
    pub conn: Connection,
}

/// The finding list query. Shared with the command layer so the Tauri command argument and the
/// SQL filter cannot drift apart.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingQuery {
    pub report_id: String,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub saved_view: Option<String>,
    #[serde(default)]
    pub page: Option<usize>,
    #[serde(default)]
    pub page_size: Option<usize>,
}

/// Upper bound on one page of findings, enforced in the query rather than at each call site.
pub const MAX_FINDING_PAGE_SIZE: usize = 500;

impl Database {
    pub fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        let mut db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        let mut db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&mut self) -> AppResult<()> {
        let transaction = self.conn.transaction()?;
        transaction.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
            INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (1, datetime('now'));

            CREATE TABLE IF NOT EXISTS profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                repository_path TEXT,
                report_id TEXT,
                scan_prefix TEXT,
                repository_prefix TEXT,
                ui_state_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS reports (
                id TEXT PRIMARY KEY,
                profile_id TEXT,
                source_name TEXT NOT NULL,
                source_path TEXT NOT NULL,
                sha256 TEXT NOT NULL UNIQUE,
                adapter_id TEXT NOT NULL,
                adapter_version TEXT NOT NULL,
                imported_at TEXT NOT NULL,
                raw_path TEXT NOT NULL,
                metadata_json TEXT NOT NULL,
                diagnostics_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS findings (
                id TEXT PRIMARY KEY,
                report_id TEXT NOT NULL,
                profile_id TEXT,
                fingerprint TEXT NOT NULL,
                engine TEXT NOT NULL,
                scanner TEXT,
                rule TEXT,
                title TEXT NOT NULL,
                severity TEXT NOT NULL,
                original_severity TEXT,
                result_status TEXT,
                triage_state TEXT,
                category TEXT NOT NULL,
                file_path TEXT,
                package_name TEXT,
                package_version TEXT,
                scan_package_version TEXT,
                line_start INTEGER,
                line_end INTEGER,
                evidence_readiness TEXT NOT NULL,
                local_task_state TEXT NOT NULL,
                raw_locator TEXT NOT NULL,
                record_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS findings_report_index ON findings(report_id);
            CREATE INDEX IF NOT EXISTS findings_fingerprint_index ON findings(fingerprint);

            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                profile_id TEXT NOT NULL,
                report_id TEXT NOT NULL,
                finding_ids_json TEXT NOT NULL,
                state TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                snapshot_json TEXT NOT NULL,
                proposal_json TEXT,
                diff TEXT,
                patch_id TEXT,
                notes TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS patches (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                diff TEXT NOT NULL,
                touched_files_json TEXT NOT NULL,
                journal_json TEXT NOT NULL,
                applied_at TEXT,
                undone_at TEXT
            );

            CREATE TABLE IF NOT EXISTS validation_runs (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                candidate_id TEXT NOT NULL,
                status TEXT NOT NULL,
                exit_code INTEGER,
                duration_ms INTEGER NOT NULL,
                stdout TEXT NOT NULL,
                stderr TEXT NOT NULL,
                snapshot_hash TEXT NOT NULL,
                started_at TEXT NOT NULL,
                note TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS comparison_items (
                id TEXT PRIMARY KEY,
                baseline_report_id TEXT NOT NULL,
                compared_report_id TEXT NOT NULL,
                payload_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS finding_notes (
                finding_id TEXT PRIMARY KEY,
                note TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn report_by_hash(&self, sha256: &str) -> AppResult<Option<ReportSummary>> {
        self.conn
            .query_row(
                "SELECT id, source_name, source_path, sha256, adapter_id, adapter_version, imported_at, metadata_json, diagnostics_json FROM reports WHERE sha256 = ?1",
                [sha256],
                report_from_row,
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn insert_report(
        &mut self,
        report: &ReportSummary,
        diagnostics: &ImportDiagnostics,
        raw_path: &str,
        findings: &[FindingRecord],
    ) -> AppResult<()> {
        if self.report_by_hash(&report.sha256)?.is_some() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO reports(id, profile_id, source_name, source_path, sha256, adapter_id, adapter_version, imported_at, raw_path, metadata_json, diagnostics_json) VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                report.id,
                report.source_name,
                report.source_path,
                report.sha256,
                report.adapter_id,
                report.adapter_version,
                report.imported_at,
                raw_path,
                json(&report.metadata)?,
                json(diagnostics)?,
            ],
        )?;
        for finding in findings {
            let summary = &finding.summary;
            tx.execute(
                "INSERT INTO findings(id, report_id, profile_id, fingerprint, engine, scanner, rule, title, severity, original_severity, result_status, triage_state, category, file_path, package_name, package_version, scan_package_version, line_start, line_end, evidence_readiness, local_task_state, raw_locator, record_json) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
                params![
                    summary.id,
                    summary.report_id,
                    summary.fingerprint,
                    summary.engine,
                    summary.scanner,
                    summary.rule,
                    summary.title,
                    summary.severity,
                    summary.original_severity,
                    summary.result_status,
                    summary.triage_state,
                    enum_json(&summary.category),
                    summary.file_path,
                    summary.package_name,
                    summary.package_version,
                    summary.scan_package_version,
                    summary.line_start,
                    summary.line_end,
                    enum_json(&summary.evidence_readiness),
                    enum_json(&summary.local_task_state),
                    summary.raw_locator,
                    json(finding)?,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn diagnostics_for_report(&self, report_id: &str) -> AppResult<Option<ImportDiagnostics>> {
        self.conn
            .query_row(
                "SELECT diagnostics_json FROM reports WHERE id = ?1",
                [report_id],
                |row| {
                    let value: String = row.get(0)?;
                    Ok(value)
                },
            )
            .optional()?
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(AppError::from)
    }

    pub fn report_summary(&self, report_id: &str) -> AppResult<Option<ReportSummary>> {
        self.conn
            .query_row(
                "SELECT id, source_name, source_path, sha256, adapter_id, adapter_version, imported_at, metadata_json, diagnostics_json FROM reports WHERE id = ?1",
                [report_id],
                report_from_row,
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn latest_report(&self) -> AppResult<Option<ReportSummary>> {
        self.conn
            .query_row(
                "SELECT id, source_name, source_path, sha256, adapter_id, adapter_version, imported_at, metadata_json, diagnostics_json FROM reports ORDER BY imported_at DESC LIMIT 1",
                [],
                report_from_row,
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn raw_path(&self, report_id: &str) -> AppResult<Option<String>> {
        self.conn
            .query_row(
                "SELECT raw_path FROM reports WHERE id = ?1",
                [report_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn finding(&self, finding_id: &str) -> AppResult<Option<FindingRecord>> {
        self.conn
            .query_row(
                "SELECT record_json, local_task_state FROM findings WHERE id = ?1",
                [finding_id],
                |row| {
                    let value: String = row.get(0)?;
                    let state: String = row.get(1)?;
                    Ok((value, state))
                },
            )
            .optional()?
            .map(|(value, state)| {
                let mut record = parse_finding_json(&value)?;
                apply_local_task_state(&mut record, &state);
                Ok::<FindingRecord, serde_json::Error>(record)
            })
            .transpose()
            .map_err(AppError::from)
    }

    pub fn findings(&self, request: &FindingQuery) -> AppResult<Vec<FindingSummary>> {
        let mut query =
            String::from("SELECT record_json, local_task_state FROM findings WHERE report_id = ?1");
        let mut args: Vec<String> = vec![request.report_id.to_owned()];
        let search = request.search.as_deref().unwrap_or_default();
        if !search.trim().is_empty() {
            query.push_str(" AND (lower(title) LIKE lower(?2) OR lower(COALESCE(file_path, '')) LIKE lower(?2) OR lower(COALESCE(package_name, '')) LIKE lower(?2) OR lower(COALESCE(rule, '')) LIKE lower(?2))");
            args.push(format!("%{}%", search.trim()));
        }
        if let Some(severity) = request.severity.as_deref() {
            query.push_str(&format!(" AND severity = ?{}", args.len() + 1));
            args.push(severity.to_owned());
        }
        if let Some(engine) = request.engine.as_deref() {
            query.push_str(&format!(" AND engine = ?{}", args.len() + 1));
            args.push(engine.to_owned());
        }
        if let Some(status) = request.status.as_deref() {
            query.push_str(&format!(" AND result_status = ?{}", args.len() + 1));
            args.push(status.to_owned());
        }
        if let Some(view) = request.saved_view.as_deref() {
            match view {
                "needs-investigation" => query.push_str(" AND local_task_state = 'investigating'"),
                "ready-to-review" => query.push_str(" AND local_task_state = 'proposalready'"),
                "patched-awaiting-validation" => {
                    query.push_str(" AND local_task_state = 'applied'")
                }
                "awaiting-rescan" => query.push_str(" AND local_task_state = 'awaitingrescan'"),
                _ => {}
            }
        }
        query.push_str(" ORDER BY CASE severity WHEN 'Critical' THEN 0 WHEN 'High' THEN 1 WHEN 'Medium' THEN 2 WHEN 'Low' THEN 3 ELSE 4 END, title LIMIT ? OFFSET ?");
        let page = request.page.unwrap_or(0);
        let page_size = request.page_size.unwrap_or(200).min(MAX_FINDING_PAGE_SIZE);
        let offset = page.saturating_mul(page_size);
        let limit_value = page_size as i64;
        let offset_value = offset as i64;
        let mut statement = self.conn.prepare(&query)?;
        let mut values: Vec<&dyn rusqlite::ToSql> =
            args.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        values.push(&limit_value);
        values.push(&offset_value);
        let rows = statement.query_map(rusqlite::params_from_iter(values), |row| {
            let value: String = row.get(0)?;
            let state: String = row.get(1)?;
            Ok((value, state))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (raw, state) = row?;
            let mut record: FindingRecord = parse_finding_json(&raw).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            apply_local_task_state(&mut record, &state);
            result.push(record.summary);
        }
        Ok(result)
    }

    pub fn all_findings_for_report(&self, report_id: &str) -> AppResult<Vec<FindingRecord>> {
        let mut statement = self.conn.prepare(
            "SELECT record_json, local_task_state FROM findings WHERE report_id = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map([report_id], |row| {
            let value: String = row.get(0)?;
            let state: String = row.get(1)?;
            Ok((value, state))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (raw, state) = row?;
            let mut record = parse_finding_json(&raw)?;
            apply_local_task_state(&mut record, &state);
            result.push(record);
        }
        Ok(result)
    }

    pub fn upsert_profile(&mut self, profile: &Profile, report_id: Option<&str>) -> AppResult<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO profiles(id, name, repository_path, report_id, scan_prefix, repository_prefix, ui_state_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8) ON CONFLICT(id) DO UPDATE SET name=excluded.name, repository_path=excluded.repository_path, report_id=excluded.report_id, scan_prefix=excluded.scan_prefix, repository_prefix=excluded.repository_prefix, ui_state_json=excluded.ui_state_json, updated_at=excluded.updated_at",
            params![profile.id, profile.name, profile.repository_path, report_id.or(profile.report_id.as_deref()), profile.scan_prefix, profile.repository_prefix, json(&profile.ui_state)?, now],
        )?;
        if let Some(report_id) = report_id.or(profile.report_id.as_deref()) {
            self.conn.execute(
                "UPDATE reports SET profile_id = ?1 WHERE id = ?2",
                params![profile.id, report_id],
            )?;
            self.conn.execute(
                "UPDATE findings SET profile_id = ?1 WHERE report_id = ?2",
                params![profile.id, report_id],
            )?;
        }
        Ok(())
    }

    pub fn profile_for_report(&self, report_id: &str) -> AppResult<Option<Profile>> {
        self.conn
            .query_row(
                "SELECT id, name, repository_path, report_id, scan_prefix, repository_prefix, ui_state_json FROM profiles WHERE report_id = ?1 ORDER BY updated_at DESC LIMIT 1",
                [report_id],
                profile_from_row,
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn last_profile(&self) -> AppResult<Option<Profile>> {
        self.conn
            .query_row(
                "SELECT id, name, repository_path, report_id, scan_prefix, repository_prefix, ui_state_json FROM profiles ORDER BY updated_at DESC LIMIT 1",
                [],
                profile_from_row,
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn save_ui_state(&mut self, profile_id: &str, state: &UiState) -> AppResult<()> {
        self.conn.execute(
            "UPDATE profiles SET ui_state_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![json(state)?, chrono::Utc::now().to_rfc3339(), profile_id],
        )?;
        Ok(())
    }

    pub fn delete_profile_data(&mut self, profile_id: &str) -> AppResult<DeletedWorkspaceData> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM profiles WHERE id = ?1)",
            [profile_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(AppError::Message("workspace profile not found".to_owned()));
        }
        let raw_paths = {
            let mut statement = self
                .conn
                .prepare("SELECT raw_path FROM reports WHERE profile_id = ?1")?;
            statement
                .query_map([profile_id], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let patch_ids = {
            let mut statement = self.conn.prepare(
                "SELECT id FROM patches WHERE task_id IN (SELECT id FROM tasks WHERE profile_id = ?1)",
            )?;
            statement
                .query_map([profile_id], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let transaction = self.conn.transaction()?;
        transaction.execute(
            "DELETE FROM comparison_items WHERE baseline_report_id IN (SELECT id FROM reports WHERE profile_id = ?1) OR compared_report_id IN (SELECT id FROM reports WHERE profile_id = ?1)",
            [profile_id],
        )?;
        transaction.execute(
            "DELETE FROM finding_notes WHERE finding_id IN (SELECT id FROM findings WHERE report_id IN (SELECT id FROM reports WHERE profile_id = ?1))",
            [profile_id],
        )?;
        transaction.execute(
            "DELETE FROM validation_runs WHERE task_id IN (SELECT id FROM tasks WHERE profile_id = ?1)",
            [profile_id],
        )?;
        transaction.execute(
            "DELETE FROM patches WHERE task_id IN (SELECT id FROM tasks WHERE profile_id = ?1)",
            [profile_id],
        )?;
        transaction.execute("DELETE FROM tasks WHERE profile_id = ?1", [profile_id])?;
        transaction.execute(
            "DELETE FROM findings WHERE report_id IN (SELECT id FROM reports WHERE profile_id = ?1)",
            [profile_id],
        )?;
        transaction.execute("DELETE FROM reports WHERE profile_id = ?1", [profile_id])?;
        transaction.execute("DELETE FROM profiles WHERE id = ?1", [profile_id])?;
        transaction.commit()?;
        Ok(DeletedWorkspaceData {
            raw_paths,
            patch_ids,
        })
    }

    pub fn insert_task(&mut self, task: &RemediationTask) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO tasks(id, profile_id, report_id, finding_ids_json, state, created_at, updated_at, snapshot_json, proposal_json, diff, patch_id, notes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![task.id, task.profile_id, task.report_id, json(&task.finding_ids)?, enum_json(&task.state), task.created_at, task.updated_at, json(&task.snapshot)?, task.proposal.as_ref().map(json).transpose()?, task.diff, task.patch_id, task.notes],
        )?;
        Ok(())
    }

    pub fn update_task_proposal(
        &mut self,
        task_id: &str,
        state: &TaskState,
        proposal: &ProposalDocument,
        diff: &str,
    ) -> AppResult<()> {
        self.conn.execute(
            "UPDATE tasks SET state = ?1, updated_at = ?2, proposal_json = ?3, diff = ?4 WHERE id = ?5",
            params![enum_json(state), chrono::Utc::now().to_rfc3339(), json(proposal)?, diff, task_id],
        )?;
        Ok(())
    }

    pub fn update_task_state(
        &mut self,
        task_id: &str,
        state: &TaskState,
        patch_id: Option<&str>,
    ) -> AppResult<()> {
        self.conn.execute(
            "UPDATE tasks SET state = ?1, updated_at = ?2, patch_id = COALESCE(?3, patch_id) WHERE id = ?4",
            params![enum_json(state), chrono::Utc::now().to_rfc3339(), patch_id, task_id],
        )?;
        // `findings.local_task_state` is the column the saved finding views filter on, so it is
        // kept in step with the task that owns the finding.
        let finding_ids: Option<String> = self
            .conn
            .query_row(
                "SELECT finding_ids_json FROM tasks WHERE id = ?1",
                [task_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(raw) = finding_ids {
            let parsed: Vec<String> = serde_json::from_str(&raw)?;
            for finding_id in parsed {
                self.conn.execute(
                    "UPDATE findings SET local_task_state = ?1 WHERE id = ?2",
                    params![enum_json(state), finding_id],
                )?;
            }
        }
        Ok(())
    }

    pub fn clear_task_patch(&mut self, task_id: &str) -> AppResult<()> {
        self.conn.execute(
            "UPDATE tasks SET patch_id = NULL, updated_at = ?1 WHERE id = ?2",
            params![chrono::Utc::now().to_rfc3339(), task_id],
        )?;
        Ok(())
    }

    pub fn task(&self, task_id: &str) -> AppResult<Option<RemediationTask>> {
        self.conn
            .query_row(
                "SELECT id, profile_id, report_id, finding_ids_json, state, created_at, updated_at, snapshot_json, proposal_json, diff, patch_id, notes FROM tasks WHERE id = ?1",
                [task_id],
                task_from_row,
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn tasks_for_profile(&self, profile_id: &str) -> AppResult<Vec<RemediationTask>> {
        let mut statement = self.conn.prepare("SELECT id, profile_id, report_id, finding_ids_json, state, created_at, updated_at, snapshot_json, proposal_json, diff, patch_id, notes FROM tasks WHERE profile_id = ?1 ORDER BY updated_at DESC")?;
        let rows = statement.query_map([profile_id], task_from_row)?;
        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row?);
        }
        Ok(tasks)
    }

    pub fn insert_patch(
        &mut self,
        patch_id: &str,
        task_id: &str,
        diff: &str,
        files: &[String],
        journal: &Value,
    ) -> AppResult<()> {
        // The patch id is derived from the task and the diff, so re-applying the same reviewed
        // change after an undo produces the same id. The row is replaced and its undo marker is
        // cleared, because the write itself has already happened by the time this is recorded.
        self.conn.execute(
            "INSERT INTO patches(id, task_id, diff, touched_files_json, journal_json, applied_at, undone_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
             ON CONFLICT(id) DO UPDATE SET task_id = excluded.task_id, diff = excluded.diff, touched_files_json = excluded.touched_files_json, journal_json = excluded.journal_json, applied_at = excluded.applied_at, undone_at = NULL",
            params![patch_id, task_id, diff, json(files)?, json(journal)?, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn patch_journal(&self, patch_id: &str) -> AppResult<Option<(String, Value)>> {
        self.conn
            .query_row(
                "SELECT task_id, journal_json FROM patches WHERE id = ?1",
                [patch_id],
                |row| {
                    let task_id: String = row.get(0)?;
                    let journal: String = row.get(1)?;
                    Ok((task_id, journal))
                },
            )
            .optional()?
            .map(|(task_id, journal)| serde_json::from_str::<Value>(&journal).map(|v| (task_id, v)))
            .transpose()
            .map_err(AppError::from)
    }

    pub fn mark_patch_undone(&mut self, patch_id: &str) -> AppResult<()> {
        self.conn.execute(
            "UPDATE patches SET undone_at = ?1 WHERE id = ?2",
            params![chrono::Utc::now().to_rfc3339(), patch_id],
        )?;
        Ok(())
    }

    pub fn insert_validation(&mut self, run: &ValidationRun) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO validation_runs(id, task_id, candidate_id, status, exit_code, duration_ms, stdout, stderr, snapshot_hash, started_at, note) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![run.id, run.task_id, run.candidate_id, enum_json(&run.status), run.exit_code, run.duration_ms as i64, run.stdout, run.stderr, run.snapshot_hash, run.started_at, run.note],
        )?;
        Ok(())
    }

    pub fn validations_for_task(&self, task_id: &str) -> AppResult<Vec<ValidationRun>> {
        let mut statement = self.conn.prepare("SELECT id, task_id, candidate_id, status, exit_code, duration_ms, stdout, stderr, snapshot_hash, started_at, note FROM validation_runs WHERE task_id = ?1 ORDER BY started_at DESC")?;
        let rows = statement.query_map([task_id], validation_from_row)?;
        let mut values = Vec::new();
        for row in rows {
            values.push(row?);
        }
        Ok(values)
    }

    pub fn save_note(&mut self, finding_id: &str, note: &str) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO finding_notes(finding_id, note, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(finding_id) DO UPDATE SET note=excluded.note, updated_at=excluded.updated_at",
            params![finding_id, note, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn note(&self, finding_id: &str) -> AppResult<Option<String>> {
        self.conn
            .query_row(
                "SELECT note FROM finding_notes WHERE finding_id = ?1",
                [finding_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(AppError::from)
    }
}

fn json<T: serde::Serialize + ?Sized>(value: &T) -> AppResult<String> {
    serde_json::to_string(value).map_err(AppError::from)
}

/// `findings.local_task_state` is the mutable column the saved views filter on, while
/// `record_json` is the immutable imported record. The column is authoritative when the two
/// disagree, which is what keeps a task's progress visible in the finding list.
fn apply_local_task_state(record: &mut FindingRecord, column: &str) {
    if let Ok(state) = serde_json::from_value::<TaskState>(Value::String(column.to_owned())) {
        record.summary.local_task_state = state;
    }
}

fn parse_finding_json(value: &str) -> Result<FindingRecord, serde_json::Error> {
    match serde_json::from_str(value) {
        Ok(record) => Ok(record),
        Err(original_error) => {
            let mut root: Value = serde_json::from_str(value)?;
            let Some(summary) = root
                .as_object_mut()
                .and_then(|object| object.remove("summary"))
            else {
                return Err(original_error);
            };
            let (Some(root), Value::Object(summary)) = (root.as_object_mut(), summary) else {
                return Err(original_error);
            };
            for (key, value) in summary {
                root.entry(key).or_insert(value);
            }
            serde_json::from_value(Value::Object(root.clone()))
        }
    }
}

fn enum_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "\"unknown\"".to_owned())
        .trim_matches('"')
        .to_owned()
}

fn report_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReportSummary> {
    let metadata: String = row.get(7)?;
    let diagnostics: String = row.get(8)?;
    let diagnostics: ImportDiagnostics = serde_json::from_str(&diagnostics).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(ReportSummary {
        id: row.get(0)?,
        source_name: row.get(1)?,
        source_path: row.get(2)?,
        sha256: row.get(3)?,
        adapter_id: row.get(4)?,
        adapter_version: row.get(5)?,
        imported_at: row.get(6)?,
        finding_count: diagnostics.parsed_instances,
        scanner_sections: diagnostics.scanner_sections,
        metadata: serde_json::from_str(&metadata).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                7,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
    })
}

fn profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Profile> {
    let ui_state: String = row.get(6)?;
    Ok(Profile {
        id: row.get(0)?,
        name: row.get(1)?,
        repository_path: row.get(2)?,
        report_id: row.get(3)?,
        scan_prefix: row.get(4)?,
        repository_prefix: row.get(5)?,
        ui_state: serde_json::from_str(&ui_state).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
    })
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RemediationTask> {
    let finding_ids: String = row.get(3)?;
    let state: String = row.get(4)?;
    let snapshot: String = row.get(7)?;
    let proposal: Option<String> = row.get(8)?;
    Ok(RemediationTask {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        report_id: row.get(2)?,
        finding_ids: serde_json::from_str(&finding_ids).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        state: serde_json::from_value(serde_json::Value::String(state))
            .unwrap_or(TaskState::Investigating),
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        snapshot: serde_json::from_str(&snapshot).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                7,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        proposal: proposal
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    8,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        diff: row.get(9)?,
        patch_id: row.get(10)?,
        notes: row.get(11)?,
    })
}

fn validation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ValidationRun> {
    let status: String = row.get(3)?;
    Ok(ValidationRun {
        id: row.get(0)?,
        task_id: row.get(1)?,
        candidate_id: row.get(2)?,
        status: serde_json::from_value(serde_json::Value::String(status))
            .unwrap_or(ValidationStatus::Unknown),
        exit_code: row.get(4)?,
        duration_ms: row.get::<_, i64>(5)? as u128,
        stdout: row.get(6)?,
        stderr: row.get(7)?,
        snapshot_hash: row.get(8)?,
        started_at: row.get(9)?,
        note: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::parse_report;

    fn seeded_database() -> (Database, String, String) {
        let mut db = Database::memory().unwrap();
        let parsed = parse_report(
            "results.json",
            "/tmp/results.json",
            br#"{"results":[{"id":"1","engine":"SAST","filePath":"src/a.ts","line":4,"severity":"High"}]}"#
                .to_vec(),
        )
        .unwrap();
        db.insert_report(
            &parsed.report,
            &parsed.diagnostics,
            "/tmp/raw.json",
            &parsed.findings,
        )
        .unwrap();
        let report_id = parsed.report.id.clone();
        let finding_id = parsed.findings[0].summary.id.clone();
        let task = RemediationTask {
            id: "task-1".to_owned(),
            profile_id: "profile-1".to_owned(),
            report_id: report_id.clone(),
            finding_ids: vec![finding_id.clone()],
            state: TaskState::Investigating,
            created_at: "now".to_owned(),
            updated_at: "now".to_owned(),
            snapshot: SnapshotManifest {
                task_id: "task-1".to_owned(),
                report_id: report_id.clone(),
                repository_path: "/tmp".to_owned(),
                branch: None,
                head_commit: None,
                captured_at: "now".to_owned(),
                files: Vec::new(),
            },
            proposal: None,
            diff: None,
            patch_id: None,
            notes: String::new(),
        };
        db.insert_task(&task).unwrap();
        (db, report_id, finding_id)
    }

    fn count_in_view(db: &Database, report_id: &str, view: &str) -> usize {
        let request = FindingQuery {
            report_id: report_id.to_owned(),
            saved_view: Some(view.to_owned()),
            page_size: Some(50),
            ..FindingQuery::default()
        };
        db.findings(&request).unwrap().len()
    }

    #[test]
    fn saved_views_follow_the_task_state_of_their_findings() {
        let (mut db, report_id, finding_id) = seeded_database();
        assert_eq!(count_in_view(&db, &report_id, "needs-investigation"), 1);
        assert_eq!(count_in_view(&db, &report_id, "ready-to-review"), 0);

        db.update_task_state("task-1", &TaskState::ProposalReady, None)
            .unwrap();
        assert_eq!(count_in_view(&db, &report_id, "needs-investigation"), 0);
        assert_eq!(count_in_view(&db, &report_id, "ready-to-review"), 1);

        db.update_task_state("task-1", &TaskState::AwaitingRescan, None)
            .unwrap();
        assert_eq!(count_in_view(&db, &report_id, "awaiting-rescan"), 1);
        assert!(matches!(
            db.finding(&finding_id)
                .unwrap()
                .unwrap()
                .summary
                .local_task_state,
            TaskState::AwaitingRescan
        ));
    }

    #[test]
    fn reapplying_the_same_patch_replaces_its_journal_row() {
        let (mut db, _, _) = seeded_database();
        let files = vec!["src/a.ts".to_owned()];
        let journal = serde_json::json!([]);
        db.insert_patch("patch-1", "task-1", "diff", &files, &journal)
            .unwrap();
        db.mark_patch_undone("patch-1").unwrap();
        // Apply → undo → apply rebuilds the same deterministic patch id, and the file write has
        // already happened by the time the journal row is recorded.
        db.insert_patch("patch-1", "task-1", "diff", &files, &journal)
            .unwrap();
        let (task_id, _) = db.patch_journal("patch-1").unwrap().unwrap();
        assert_eq!(task_id, "task-1");
        let undone_at: Option<String> = db
            .conn
            .query_row(
                "SELECT undone_at FROM patches WHERE id = 'patch-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(undone_at.is_none());
    }
}
