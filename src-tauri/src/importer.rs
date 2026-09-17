use std::collections::BTreeSet;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};
use crate::models::*;

pub const MAX_REPORT_BYTES: u64 = 100 * 1024 * 1024;
pub const ADAPTER_VERSION: &str = "1.0.0";

struct ParseAccumulator {
    findings: Vec<FindingRecord>,
    unsupported: Vec<UnsupportedRecord>,
    malformed: Vec<MalformedRecord>,
    sections: BTreeSet<String>,
    declared_counts: Vec<DeclaredCount>,
    warnings: Vec<String>,
}

impl ParseAccumulator {
    fn new() -> Self {
        Self {
            findings: Vec::new(),
            unsupported: Vec::new(),
            malformed: Vec::new(),
            sections: BTreeSet::new(),
            declared_counts: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

pub struct ParsedReport {
    pub report: ReportSummary,
    pub diagnostics: ImportDiagnostics,
    pub findings: Vec<FindingRecord>,
    pub raw_bytes: Vec<u8>,
}

#[derive(Clone, Default)]
struct GroupContext {
    engine: Option<String>,
    scanner: Option<String>,
    rule: Option<String>,
    query_id: Option<String>,
    query_name: Option<String>,
    severity: Option<String>,
}

impl GroupContext {
    fn from_object(object: &Map<String, Value>) -> Self {
        Self {
            engine: first_string(object, &["engine", "scanType", "type"]),
            scanner: first_string(object, &["scanner", "product", "source"]),
            rule: first_string(object, &["rule", "ruleId", "checkId", "check", "policy"]),
            query_id: first_string(object, &["queryId", "query_id"]),
            query_name: first_string(object, &["queryName", "query"]),
            severity: first_string(object, &["severity", "originalSeverity", "priority"]),
        }
    }

    fn inherit(self, parent: &Self) -> Self {
        Self {
            engine: self.engine.or_else(|| parent.engine.clone()),
            scanner: self.scanner.or_else(|| parent.scanner.clone()),
            rule: self.rule.or_else(|| parent.rule.clone()),
            query_id: self.query_id.or_else(|| parent.query_id.clone()),
            query_name: self.query_name.or_else(|| parent.query_name.clone()),
            severity: self.severity.or_else(|| parent.severity.clone()),
        }
    }
}

pub fn parse_report(
    source_name: &str,
    source_path: &str,
    bytes: Vec<u8>,
) -> AppResult<ParsedReport> {
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err(AppError::OversizedReport(MAX_REPORT_BYTES));
    }
    let root: Value = serde_json::from_slice(&bytes)?;
    let sha256 = sha256_hex(&bytes);
    let report_id = format!("report-{}", &sha256[..24]);
    let imported_at = chrono::Utc::now().to_rfc3339();
    let mut accumulator = ParseAccumulator::new();
    let (format, adapter_id) = if let Some(object) = root.as_object() {
        let grouped_keys = ["scanResults", "scaScanResults", "iacScanResults"];
        if grouped_keys.iter().any(|key| object.contains_key(*key)) {
            collect_root_declared_counts(object, &mut accumulator.declared_counts);
            for key in grouped_keys {
                if let Some(value) = object.get(key) {
                    accumulator.sections.insert(key.to_owned());
                    collect_declared_counts(
                        value,
                        &format!("/{key}"),
                        &mut accumulator.declared_counts,
                        0,
                    );
                    if value.is_null() {
                        accumulator
                            .warnings
                            .push(format!("/{key} is null; no instances were discarded."));
                    } else {
                        collect_grouped(
                            value,
                            key_to_category(key),
                            &format!("/{key}"),
                            &GroupContext::default(),
                            &mut accumulator,
                        );
                    }
                }
            }
            collect_unknown_top_level(object, &mut accumulator);
            (
                "CXone grouped web report".to_owned(),
                "cxone-grouped".to_owned(),
            )
        } else if object.get("results").is_some() {
            accumulator.sections.insert("results".to_owned());
            collect_root_declared_counts(object, &mut accumulator.declared_counts);
            collect_declared_counts(
                object.get("results").unwrap_or(&Value::Null),
                "/results",
                &mut accumulator.declared_counts,
                0,
            );
            match object.get("results") {
                Some(Value::Array(items)) => {
                    for (index, item) in items.iter().enumerate() {
                        collect_result_item(item, &format!("/results/{index}"), &mut accumulator);
                    }
                }
                Some(Value::Null) => accumulator
                    .warnings
                    .push("/results is null; no instances were discarded.".to_owned()),
                Some(other) => accumulator.malformed.push(MalformedRecord {
                    locator: "/results".to_owned(),
                    reason: format!("expected an array, found {}", json_type(other)),
                }),
                None => {}
            }
            collect_unknown_top_level(object, &mut accumulator);
            (
                "CXone result-oriented report".to_owned(),
                "cxone-results".to_owned(),
            )
        } else if looks_like_summary_report(object) {
            accumulator.sections.insert("summary".to_owned());
            accumulator.unsupported.push(UnsupportedRecord {
                locator: "/".to_owned(),
                reason: "This project summary export contains aggregated counts but no machine-readable individual findings; it was retained as a summary-only import.".to_owned(),
                preview: masked_preview(&root),
            });
            (
                "Checkmarx project summary report".to_owned(),
                "checkmarx-summary".to_owned(),
            )
        } else {
            accumulator.sections.insert("unrecognized".to_owned());
            accumulator.unsupported.push(UnsupportedRecord { locator: "/".to_owned(), reason: "No supported scanResults/scaScanResults/iacScanResults or top-level results array was found.".to_owned(), preview: masked_preview(&root) });
            (
                "Unsupported JSON shape".to_owned(),
                "unsupported".to_owned(),
            )
        }
    } else {
        accumulator.malformed.push(MalformedRecord {
            locator: "/".to_owned(),
            reason: "top-level JSON value must be an object".to_owned(),
        });
        ("Malformed JSON report".to_owned(), "unsupported".to_owned())
    };

    let metadata = extract_metadata(&root, &accumulator.declared_counts);
    let mut counts = ParsedCounts::default();
    for finding in &accumulator.findings {
        match finding.summary.category {
            FindingCategory::Sast => counts.sast += 1,
            FindingCategory::Sca => counts.sca += 1,
            FindingCategory::Iac => counts.iac += 1,
            FindingCategory::Unknown => counts.unknown += 1,
        }
    }
    let count_mismatches = compare_declared_counts(
        &accumulator.declared_counts,
        &counts,
        accumulator.findings.len(),
    );
    let mut evidence_notes = Vec::new();
    if accumulator.findings.is_empty()
        && accumulator.unsupported.is_empty()
        && accumulator.malformed.is_empty()
    {
        evidence_notes.push(
            "The report parsed zero individual findings. This is distinct from a failed import."
                .to_owned(),
        );
    }
    if accumulator
        .findings
        .iter()
        .any(|finding| finding.summary.category == FindingCategory::Sast && finding.nodes.len() < 2)
    {
        evidence_notes.push("At least one SAST finding has fewer than two reported flow nodes; a full reported source-to-sink flow is unavailable.".to_owned());
    }
    if !count_mismatches.is_empty() {
        evidence_notes.push("Declared report totals and parsed individual instances differ; query-group totals are not treated as finding instances.".to_owned());
    }
    if format == "Unsupported JSON shape" {
        evidence_notes.push("PDF, summary-only, SARIF, and unrecognized JSON exports are not treated as complete machine-readable findings reports.".to_owned());
    }
    let diagnostics = ImportDiagnostics {
        format: format.clone(),
        adapter_id: adapter_id.clone(),
        adapter_version: ADAPTER_VERSION.to_owned(),
        scanner_sections: accumulator.sections.into_iter().collect(),
        parsed_instances: accumulator.findings.len(),
        parsed_by_engine: counts,
        unsupported_records: accumulator.unsupported,
        malformed_records: accumulator.malformed,
        count_mismatches,
        evidence_notes,
        warnings: accumulator.warnings,
    };
    let report = ReportSummary {
        id: report_id,
        source_name: source_name.to_owned(),
        source_path: source_path.to_owned(),
        sha256,
        adapter_id,
        adapter_version: ADAPTER_VERSION.to_owned(),
        imported_at,
        finding_count: diagnostics.parsed_instances,
        scanner_sections: diagnostics.scanner_sections.clone(),
        metadata,
    };
    let mut findings = accumulator.findings;
    attach_report_id(&mut findings, &report.id);
    Ok(ParsedReport {
        report,
        diagnostics,
        findings,
        raw_bytes: bytes,
    })
}

fn collect_grouped(
    value: &Value,
    category: FindingCategory,
    locator: &str,
    inherited_context: &GroupContext,
    accumulator: &mut ParseAccumulator,
) {
    match value {
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_grouped(
                    item,
                    category.clone(),
                    &format!("{locator}/{index}"),
                    inherited_context,
                    accumulator,
                );
            }
        }
        Value::Object(object) => {
            let context = GroupContext::from_object(object).inherit(inherited_context);
            if looks_like_finding(object, &category) {
                match normalize_finding(object, category.clone(), locator, Some(&context)) {
                    Ok(finding) => accumulator.findings.push(finding),
                    Err(reason) => accumulator.malformed.push(MalformedRecord {
                        locator: locator.to_owned(),
                        reason,
                    }),
                }
                return;
            }
            let mut visited_known = false;
            for key in [
                "results",
                "findings",
                "issues",
                "records",
                "vulnerabilityResults",
                "queryResults",
            ] {
                if let Some(child) = object.get(key) {
                    visited_known = true;
                    if child.is_null() {
                        accumulator.warnings.push(format!(
                            "{locator}/{key} is null; no instances were discarded."
                        ));
                    } else {
                        collect_grouped(
                            child,
                            category.clone(),
                            &format!("{locator}/{key}"),
                            &context,
                            accumulator,
                        );
                    }
                }
            }
            if !visited_known {
                let supported_group_keys = [
                    "queryId",
                    "queryName",
                    "severity",
                    "total",
                    "count",
                    "engine",
                    "scanner",
                ];
                if object
                    .keys()
                    .any(|key| supported_group_keys.contains(&key.as_str()))
                {
                    accumulator.unsupported.push(UnsupportedRecord { locator: locator.to_owned(), reason: "Recognized grouped section item did not contain an individual finding location or package/resource identity.".to_owned(), preview: masked_preview(&Value::Object(object.clone())) });
                } else {
                    accumulator.unsupported.push(UnsupportedRecord { locator: locator.to_owned(), reason: "Unsupported record in a recognized section; preserved in the raw snapshot.".to_owned(), preview: masked_preview(&Value::Object(object.clone())) });
                }
            }
        }
        other => accumulator.malformed.push(MalformedRecord {
            locator: locator.to_owned(),
            reason: format!("expected an object or array, found {}", json_type(other)),
        }),
    }
}

fn collect_result_item(value: &Value, locator: &str, accumulator: &mut ParseAccumulator) {
    let Some(object) = value.as_object() else {
        if value.is_null() {
            accumulator.warnings.push(format!(
                "{locator} is null; it was not counted as a finding."
            ));
        } else {
            accumulator.malformed.push(MalformedRecord {
                locator: locator.to_owned(),
                reason: format!("result must be an object, found {}", json_type(value)),
            });
        }
        return;
    };
    let category = category_from_object(object);
    if !looks_like_finding(object, &category) {
        accumulator.unsupported.push(UnsupportedRecord { locator: locator.to_owned(), reason: "Result object has no supported individual finding identity, location, package, or resource evidence.".to_owned(), preview: masked_preview(value) });
        return;
    }
    match normalize_finding(object, category, locator, None) {
        Ok(finding) => accumulator.findings.push(finding),
        Err(reason) => accumulator.malformed.push(MalformedRecord {
            locator: locator.to_owned(),
            reason,
        }),
    }
}

fn normalize_finding(
    object: &Map<String, Value>,
    category: FindingCategory,
    locator: &str,
    grouped_context: Option<&GroupContext>,
) -> Result<FindingRecord, String> {
    let engine = first_string(object, &["engine", "scanner", "scanType", "type"])
        .or_else(|| grouped_context.and_then(|context| context.engine.clone()))
        .unwrap_or_else(|| match category {
            FindingCategory::Sast => "SAST".to_owned(),
            FindingCategory::Sca => "SCA".to_owned(),
            FindingCategory::Iac => "IaC".to_owned(),
            FindingCategory::Unknown => "Unknown".to_owned(),
        });
    let scanner = first_string(object, &["scanner", "product", "source"])
        .or_else(|| grouped_context.and_then(|context| context.scanner.clone()));
    let rule = first_string(
        object,
        &["rule", "queryId", "checkId", "ruleId", "check", "policy"],
    )
    .or_else(|| first_string(object, &["name"]))
    .or_else(|| grouped_context.and_then(|context| context.rule.clone()))
    .or_else(|| grouped_context.and_then(|context| context.query_id.clone()));
    let raw_severity = first_string(object, &["severity", "originalSeverity", "priority"])
        .or_else(|| grouped_context.and_then(|context| context.severity.clone()));
    let severity = normalize_severity(raw_severity.as_deref());
    let result_status = first_string(object, &["resultStatus", "status", "state"]);
    let triage_state = first_string(object, &["triageState", "triage", "state"]);
    let description = first_string(object, &["description", "details", "message"]);
    let recommendation = first_string(
        object,
        &["recommendation", "remediation", "suggestion", "fix"],
    );
    let cwe = first_string(object, &["cwe", "cweId", "cweName"]);
    let query_id = first_string(object, &["queryId", "query_id"])
        .or_else(|| grouped_context.and_then(|context| context.query_id.clone()));
    let query_name = first_string(object, &["queryName", "query"])
        .or_else(|| grouped_context.and_then(|context| context.query_name.clone()));
    let evidence_links = string_array(object, &["evidenceLinks", "links", "references", "urls"]);
    let nodes = extract_nodes(&Value::Object(object.clone()), locator);
    let file_path = first_string(
        object,
        &["filePath", "fileName", "path", "file", "sourceFile"],
    )
    .or_else(|| nodes.iter().find_map(|node| node.path.clone()));
    let line_start = first_number(object, &["line", "lineNumber", "startLine", "lineStart"])
        .or_else(|| nodes.iter().find_map(|node| node.line_start));
    let line_end = first_number(object, &["endLine", "lineEnd"]).or(line_start);
    let package_name = first_string(
        object,
        &[
            "packageName",
            "package",
            "dependency",
            "artifact",
            "component",
        ],
    );
    let scan_package_version = first_string(
        object,
        &[
            "packageVersion",
            "version",
            "installedVersion",
            "currentVersion",
        ],
    );
    let package_version = scan_package_version.clone();
    let ecosystem = first_string(object, &["ecosystem", "packageManager", "language"]);
    let affected_range = first_string(
        object,
        &[
            "affectedRange",
            "affectedVersions",
            "vulnerableVersionRange",
        ],
    );
    let fixed_range = first_string(
        object,
        &[
            "fixedRange",
            "fixedVersion",
            "fixedVersions",
            "upgradeVersion",
        ],
    );
    let dependency_paths = string_array(object, &["dependencyPaths", "paths", "dependencyPath"]);
    let reachability = first_string(object, &["reachability", "reachable", "isReachable"]);
    let resource = first_string(object, &["resource", "resourceName", "address", "target"]);
    let iac_rule = first_string(object, &["rule", "ruleId", "checkId", "queryId"]);
    let expected_value = first_string(
        object,
        &["expected", "expectedValue", "expectedValueDescription"],
    );
    let actual_value = first_string(object, &["actual", "actualValue", "value"]);
    let provider = first_string(object, &["provider", "cloudProvider"]);
    let context = first_string(object, &["context", "fileContext", "module"]);
    let reported_snippet = first_string(
        object,
        &["snippet", "codeSnippet", "sourceSnippet", "code", "source"],
    )
    .or_else(|| nodes.iter().find_map(|node| node.snippet.clone()));
    if category == FindingCategory::Unknown
        && file_path.is_none()
        && package_name.is_none()
        && resource.is_none()
    {
        return Err("unknown engine branch has no navigable evidence".to_owned());
    }
    let explicit_id = first_string(
        object,
        &["id", "resultId", "findingId", "similarityId", "fingerprint"],
    );
    let fingerprint = explicit_id
        .map(|id| format!("{engine}:{id}"))
        .unwrap_or_else(|| {
            let seed = format!(
                "cxview-fp-v1|{engine}|{}|{}|{}|{}|{}|{}",
                rule.clone().unwrap_or_default(),
                file_path.clone().unwrap_or_default(),
                line_start.unwrap_or_default(),
                package_name.clone().unwrap_or_default(),
                scan_package_version.clone().unwrap_or_default(),
                query_id.clone().unwrap_or_default()
            );
            format!("cxview-fp-v1:{}", &sha256_hex(seed.as_bytes())[..32])
        });
    let id = format!(
        "finding-{}",
        &sha256_hex(format!("{}|{}|{}", fingerprint, locator, category_name(&category)).as_bytes())
            [..32]
    );
    let title = first_string(
        object,
        &[
            "title",
            "name",
            "message",
            "queryName",
            "rule",
            "packageName",
            "resource",
        ],
    )
    .or_else(|| grouped_context.and_then(|context| context.query_name.clone()))
    .unwrap_or_else(|| format!("{} finding", category_name(&category)));
    let readiness = evidence_readiness(
        &category,
        file_path.is_some(),
        line_start.is_some(),
        package_name.is_some(),
        resource.is_some(),
        !nodes.is_empty(),
    );
    let summary = FindingSummary {
        id,
        report_id: String::new(),
        fingerprint,
        engine,
        scanner,
        rule,
        title,
        severity,
        original_severity: raw_severity,
        result_status,
        triage_state,
        category,
        file_path,
        package_name,
        package_version,
        scan_package_version,
        line_start,
        line_end,
        evidence_readiness: readiness,
        local_task_state: TaskState::Investigating,
        raw_locator: locator.to_owned(),
    };
    Ok(FindingRecord {
        summary,
        description,
        recommendation,
        cwe,
        query_id,
        query_name,
        evidence_links,
        nodes,
        reported_snippet,
        advisory_aliases: string_array(object, &["aliases", "advisoryAliases", "identifiers"]),
        ecosystem,
        affected_range,
        fixed_range,
        dependency_paths,
        reachability,
        resource,
        iac_rule,
        expected_value,
        actual_value,
        provider,
        context,
        raw: Value::Object(object.clone()),
    })
}

pub fn attach_report_id(findings: &mut [FindingRecord], report_id: &str) {
    for finding in findings {
        finding.summary.report_id = report_id.to_owned();
        let record_id = finding.summary.id.clone();
        finding.summary.id = format!(
            "finding-{}",
            &sha256_hex(format!("{report_id}|{record_id}").as_bytes())[..32]
        );
    }
}

fn extract_nodes(value: &Value, locator: &str) -> Vec<NodeEvidence> {
    let mut result = Vec::new();
    collect_nodes_recursive(value, locator, &mut result, 0);
    result
}

fn collect_nodes_recursive(
    value: &Value,
    locator: &str,
    result: &mut Vec<NodeEvidence>,
    depth: usize,
) {
    if depth > 12 || result.len() >= 500 {
        return;
    }
    match value {
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_nodes_recursive(item, &format!("{locator}/{index}"), result, depth + 1);
            }
        }
        Value::Object(object) => {
            let path = first_string(
                object,
                &["filePath", "fileName", "path", "file", "sourceFile"],
            );
            let line_start =
                first_number(object, &["line", "lineNumber", "startLine", "lineStart"]);
            let line_end = first_number(object, &["endLine", "lineEnd"]).or(line_start);
            let has_node_identity = path.is_some()
                || line_start.is_some()
                || object.contains_key("snippet")
                || object.contains_key("codeSnippet");
            if has_node_identity && (path.is_some() || line_start.is_some()) {
                result.push(NodeEvidence {
                    order: result.len(),
                    path,
                    line_start,
                    line_end,
                    column_start: first_number(
                        object,
                        &["column", "columnNumber", "startColumn", "colStart"],
                    ),
                    column_end: first_number(object, &["endColumn", "colEnd"]),
                    role: first_string(object, &["role", "stage", "nodeType", "kind"]),
                    source_sink: first_string(object, &["sourceSink", "sourceOrSink", "taintType"]),
                    snippet: first_string(
                        object,
                        &["snippet", "codeSnippet", "sourceSnippet", "code"],
                    ),
                    locator: locator.to_owned(),
                });
            }
            for (key, child) in object {
                if [
                    "nodes",
                    "paths",
                    "locations",
                    "codeFlow",
                    "dataflow",
                    "steps",
                    "source",
                    "sink",
                    "results",
                ]
                .contains(&key.as_str())
                {
                    collect_nodes_recursive(child, &format!("{locator}/{key}"), result, depth + 1);
                }
            }
        }
        _ => {}
    }
}

fn looks_like_summary_report(object: &Map<String, Value>) -> bool {
    let has_summary = [
        "reportType",
        "header",
        "projectsOverview",
        "bySeverity",
        "byState",
        "byStatus",
        "byScanner",
        "languageOverview",
        "resultsOverview",
        "topTenVulnerabilityType",
    ]
    .iter()
    .any(|key| object.contains_key(*key));
    let has_project_list = object
        .get("projectsOverview")
        .and_then(Value::as_object)
        .and_then(|overview| overview.get("projectsList"))
        .and_then(Value::as_array)
        .is_some();
    let has_summary_metrics = object
        .get("bySeverity")
        .and_then(Value::as_object)
        .is_some()
        || object
            .get("resultsDistributionByStatus")
            .and_then(Value::as_object)
            .is_some();
    has_summary || (has_project_list && has_summary_metrics)
}

fn collect_unknown_top_level(object: &Map<String, Value>, accumulator: &mut ParseAccumulator) {
    let known = [
        "scanResults",
        "scaScanResults",
        "iacScanResults",
        "results",
        "metadata",
        "project",
        "scan",
        "summary",
        "filters",
        "status",
        "scope",
        "engines",
        "engine",
        "version",
        "format",
        "timestamp",
        "branch",
        "commit",
    ];
    for (key, value) in object {
        if known.contains(&key.as_str()) || value.is_null() || !value.is_array() {
            continue;
        }
        if let Some(items) = value.as_array() {
            for (index, item) in items.iter().enumerate() {
                accumulator.unsupported.push(UnsupportedRecord { locator: format!("/{key}/{index}"), reason: format!("Top-level array '{key}' is not a supported scanner branch; preserved for raw inspection."), preview: masked_preview(item) });
            }
        }
    }
}

/// Collects report-level totals such as `declaredTotal`. Only direct scalar members of the
/// root object are read: descending into scanner branches here would treat a group or
/// unsupported-branch total as a report-wide declaration.
fn collect_root_declared_counts(object: &Map<String, Value>, result: &mut Vec<DeclaredCount>) {
    for (key, value) in object {
        let lower = key.to_ascii_lowercase();
        if (lower == "count"
            || lower == "total"
            || lower.ends_with("count")
            || lower.ends_with("total"))
            && value.as_u64().is_some()
        {
            result.push(DeclaredCount {
                label: key.clone(),
                value: value.as_u64().unwrap_or_default() as usize,
                locator: format!("/{key}"),
            });
        }
    }
}

fn collect_declared_counts(
    value: &Value,
    locator: &str,
    result: &mut Vec<DeclaredCount>,
    depth: usize,
) {
    if depth > 2 {
        return;
    }
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                let lower = key.to_ascii_lowercase();
                if (lower == "count"
                    || lower == "total"
                    || lower.ends_with("count")
                    || lower.ends_with("total"))
                    && value.as_u64().is_some()
                {
                    result.push(DeclaredCount {
                        label: key.clone(),
                        value: value.as_u64().unwrap_or_default() as usize,
                        locator: format!("{locator}/{key}"),
                    });
                } else if value.is_object() || value.is_array() {
                    collect_declared_counts(value, &format!("{locator}/{key}"), result, depth + 1);
                }
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().take(100).enumerate() {
                collect_declared_counts(item, &format!("{locator}/{index}"), result, depth + 1);
            }
        }
        _ => {}
    }
}

fn compare_declared_counts(
    declared: &[DeclaredCount],
    counts: &ParsedCounts,
    total: usize,
) -> Vec<CountMismatch> {
    declared
        .iter()
        .filter_map(|item| {
            let lower = item.label.to_ascii_lowercase();
            let parsed = if let Some(scoped) = section_scope(&item.locator, counts, total) {
                // A total declared inside a scanner section describes that section's own
                // query group. Comparing it against every parsed instance would report a
                // mismatch that the report never claimed.
                scoped
            } else if lower.contains("sast") {
                counts.sast
            } else if lower.contains("sca") {
                counts.sca
            } else if lower.contains("iac") {
                counts.iac
            } else if lower.contains("count")
                || lower.contains("total")
                || lower.contains("finding")
                || lower.contains("result")
            {
                total
            } else {
                return None;
            };
            (parsed != item.value).then(|| CountMismatch {
                label: item.label.clone(),
                declared: item.value,
                parsed,
                locator: item.locator.clone(),
            })
        })
        .collect()
}

fn section_scope(locator: &str, counts: &ParsedCounts, total: usize) -> Option<usize> {
    match locator.trim_start_matches('/').split('/').next() {
        Some("scanResults") => Some(counts.sast),
        Some("scaScanResults") => Some(counts.sca),
        Some("iacScanResults") => Some(counts.iac),
        Some("results") => Some(total),
        _ => None,
    }
}

fn extract_metadata(root: &Value, declared_counts: &[DeclaredCount]) -> ReportMetadata {
    let object = root.as_object();
    let get = |keys: &[&str]| object.and_then(|map| first_string(map, keys));
    let project_details = object
        .and_then(|map| map.get("projectsOverview"))
        .and_then(Value::as_object)
        .and_then(|overview| overview.get("projectsList"))
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_object);
    let included_engines = object
        .map(|map| string_array(map, &["engines", "includedEngines", "scanners"]))
        .unwrap_or_default();
    ReportMetadata {
        project_id: get(&["projectId", "projectID"])
            .or_else(|| project_details.and_then(|project| first_string(project, &["projectId", "projectID"])))
            .or_else(|| object.and_then(|map| map.get("project").and_then(as_string_from_value))),
        project_name: get(&["projectName"])
            .or_else(|| project_details.and_then(|project| first_string(project, &["projectName", "name"])))
            .or_else(|| object.and_then(|map| map.get("project").and_then(as_name_from_value))),
        scan_id: get(&["scanId", "scanID", "id"]),
        branch: get(&["branch", "branchName"]).or_else(|| {
            project_details.and_then(|project| {
                first_string(project, &["branch", "branchName"]).or_else(|| {
                    project.get("projectBranchesOverview")
                        .and_then(Value::as_array)
                        .and_then(|branches| branches.first())
                        .and_then(Value::as_object)
                        .and_then(|branch| first_string(branch, &["branchName", "branch"]))
                })
            })
        }),
        commit: get(&["commit", "commitId", "commitSha"]),
        timestamp: get(&["timestamp", "scanTimestamp", "createdAt", "date"]),
        included_engines,
        filters: object.and_then(|map| map.get("filters").cloned()),
        scan_status: get(&["scanStatus", "status"]),
        completeness: get(&["completeness", "scanCompleteness"]),
        scope: object.and_then(|map| map.get("scope").cloned()),
        declared_counts: declared_counts.to_vec(),
    }
}

fn key_to_category(key: &str) -> FindingCategory {
    match key {
        "scanResults" => FindingCategory::Sast,
        "scaScanResults" => FindingCategory::Sca,
        "iacScanResults" => FindingCategory::Iac,
        _ => FindingCategory::Unknown,
    }
}

fn category_from_object(object: &Map<String, Value>) -> FindingCategory {
    let value = first_string(
        object,
        &["engine", "scanner", "scanType", "type", "category"],
    )
    .unwrap_or_default()
    .to_ascii_lowercase();
    if value.contains("sast") || value.contains("code") {
        FindingCategory::Sast
    } else if value.contains("sca") || value.contains("dependency") || value.contains("package") {
        FindingCategory::Sca
    } else if value.contains("iac")
        || value.contains("infrastructure")
        || value.contains("terraform")
        || value.contains("kubernetes")
    {
        FindingCategory::Iac
    } else {
        FindingCategory::Unknown
    }
}

fn category_name(category: &FindingCategory) -> &'static str {
    match category {
        FindingCategory::Sast => "SAST",
        FindingCategory::Sca => "SCA",
        FindingCategory::Iac => "IaC",
        FindingCategory::Unknown => "Unknown",
    }
}

fn looks_like_finding(object: &Map<String, Value>, category: &FindingCategory) -> bool {
    let has_location = first_string(
        object,
        &["filePath", "fileName", "path", "file", "sourceFile"],
    )
    .is_some()
        || object.contains_key("locations")
        || object.contains_key("nodes")
        || object.contains_key("codeFlow");
    let has_package = first_string(
        object,
        &[
            "packageName",
            "package",
            "dependency",
            "artifact",
            "component",
        ],
    )
    .is_some();
    let has_resource =
        first_string(object, &["resource", "resourceName", "address", "target"]).is_some();
    let has_identity = first_string(
        object,
        &[
            "id",
            "resultId",
            "findingId",
            "queryId",
            "ruleId",
            "checkId",
        ],
    )
    .is_some();
    let has_nested_results = [
        "results",
        "findings",
        "issues",
        "records",
        "vulnerabilityResults",
        "queryResults",
    ]
    .iter()
    .any(|key| object.contains_key(*key));
    has_location
        || has_package
        || has_resource
        || (has_identity
            && *category != FindingCategory::Unknown
            && object.contains_key("severity")
            && !has_nested_results)
}

fn evidence_readiness(
    category: &FindingCategory,
    file: bool,
    line: bool,
    package: bool,
    resource: bool,
    nodes: bool,
) -> EvidenceReadiness {
    match category {
        FindingCategory::Sast => {
            if file && line && nodes {
                EvidenceReadiness::Ready
            } else if file || nodes {
                EvidenceReadiness::Partial
            } else {
                EvidenceReadiness::Missing
            }
        }
        FindingCategory::Sca => {
            if package {
                EvidenceReadiness::Ready
            } else {
                EvidenceReadiness::Missing
            }
        }
        FindingCategory::Iac => {
            if resource {
                EvidenceReadiness::Ready
            } else if file {
                EvidenceReadiness::Partial
            } else {
                EvidenceReadiness::Missing
            }
        }
        FindingCategory::Unknown => {
            if file || package || resource {
                EvidenceReadiness::Partial
            } else {
                EvidenceReadiness::Missing
            }
        }
    }
}

fn normalize_severity(value: Option<&str>) -> String {
    let normalized = value.unwrap_or("Unknown").trim().to_ascii_lowercase();
    match normalized.as_str() {
        "critical" | "crit" | "5" => "Critical",
        "high" | "4" => "High",
        "medium" | "moderate" | "3" => "Medium",
        "low" | "2" => "Low",
        "informational" | "info" | "informative" | "1" => "Informational",
        _ => "Unknown",
    }
    .to_owned()
}

fn first_string(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = object.get(*key).and_then(as_string_from_value) {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn as_string_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn as_name_from_value(value: &Value) -> Option<String> {
    value
        .as_object()
        .and_then(|object| first_string(object, &["name", "id", "projectName"]))
}

fn first_number(object: &Map<String, Value>, keys: &[&str]) -> Option<u32> {
    keys.iter().find_map(|key| {
        object.get(*key).and_then(|value| {
            value
                .as_u64()
                .map(|number| number as u32)
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
    })
}

fn string_array(object: &Map<String, Value>, keys: &[&str]) -> Vec<String> {
    for key in keys {
        if let Some(value) = object.get(*key) {
            if let Some(values) = value.as_array() {
                return values.iter().filter_map(as_string_from_value).collect();
            }
            if let Some(value) = as_string_from_value(value) {
                return vec![value];
            }
        }
    }
    Vec::new()
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn masked_preview(value: &Value) -> String {
    let mut text = serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_owned());
    for key in [
        "password",
        "token",
        "secret",
        "apiKey",
        "clientSecret",
        "credential",
    ] {
        let needle = format!("\"{key}\":\"");
        let mut cursor = 0;
        while let Some(start) = text[cursor..].find(&needle) {
            let start = cursor + start + needle.len();
            let Some(end) = text[start..].find('"').map(|end| start + end) else {
                break;
            };
            text.replace_range(start..end, "[masked]");
            // The replacement is not the same length as the masked value, so the next
            // search resumes from the end of the replacement rather than from an offset
            // measured against the pre-replacement text.
            cursor = start + "[masked]".len();
        }
    }
    if text.len() > 320 {
        // The preview is capped by byte length, and the cut is moved back to the nearest
        // character boundary so an untrusted multi-byte value cannot panic the truncation.
        let mut end = 320;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    text
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_fixture_preserves_sast_paths_and_branches() {
        let bytes = br#"{
          "projectId":"p1",
          "scanResults":[{"queryId":"DOM-XSS","severity":"High","results":[{"id":"x1","nodes":[{"fileName":"src/App.tsx","line":8,"role":"source","snippet":"location.hash"},{"fileName":"src/App.tsx","line":10,"role":"sink","snippet":"dangerouslySetInnerHTML"}]}]}],
          "scaScanResults":[{"results":[{"id":"sca1","packageName":"lodash","packageVersion":"4.17.20","severity":"Medium","affectedVersions":"<4.17.21","fixedVersions":"4.17.21"}]}],
          "iacScanResults":[{"results":[{"id":"iac1","resource":"aws_s3_bucket.assets","rule":"PUBLIC_BUCKET","expectedValue":"private","actualValue":"public"}]}]
        }"#.to_vec();
        let parsed = parse_report("grouped.json", "/tmp/grouped.json", bytes).unwrap();
        assert_eq!(parsed.findings.len(), 3);
        let sast = parsed
            .findings
            .iter()
            .find(|finding| finding.summary.category == FindingCategory::Sast)
            .unwrap();
        assert_eq!(sast.nodes.len(), 2);
        assert_eq!(sast.summary.severity, "High");
        assert_eq!(sast.summary.original_severity.as_deref(), Some("High"));
        assert_eq!(sast.summary.rule.as_deref(), Some("DOM-XSS"));
        assert_eq!(sast.query_name, None);
        assert_eq!(parsed.diagnostics.parsed_by_engine.sca, 1);
        assert_eq!(parsed.diagnostics.parsed_by_engine.iac, 1);
    }

    #[test]
    fn checked_in_grouped_fixture_is_the_compatibility_contract() {
        let parsed = parse_report(
            "grouped-cxone.json",
            "fixtures/synthetic/grouped-cxone.json",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/synthetic/grouped-cxone.json"
            ))
            .to_vec(),
        )
        .unwrap();
        assert_eq!(parsed.diagnostics.parsed_by_engine.sast, 1);
        assert_eq!(parsed.diagnostics.parsed_by_engine.sca, 1);
        assert_eq!(parsed.diagnostics.parsed_by_engine.iac, 1);
        let sast = parsed
            .findings
            .iter()
            .find(|finding| finding.summary.category == FindingCategory::Sast)
            .unwrap();
        assert_eq!(sast.summary.severity, "High");
        assert_eq!(sast.summary.rule.as_deref(), Some("CXSAST-RAW-HTML"));
        assert_eq!(
            sast.query_name.as_deref(),
            Some("Untrusted data in raw HTML")
        );
        assert!(!parsed.diagnostics.unsupported_records.is_empty());
        assert!(!parsed.diagnostics.count_mismatches.is_empty());
    }

    #[test]
    fn checked_in_result_fixture_keeps_unknown_branch_values() {
        let parsed = parse_report(
            "result-oriented-cxone.json",
            "fixtures/synthetic/result-oriented-cxone.json",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/synthetic/result-oriented-cxone.json"
            ))
            .to_vec(),
        )
        .unwrap();
        assert_eq!(parsed.diagnostics.parsed_instances, 3);
        assert_eq!(parsed.findings[2].summary.severity, "Unknown");
        assert_eq!(
            parsed.findings[2].summary.category,
            FindingCategory::Unknown
        );
    }

    #[test]
    fn checked_in_grouped_fixture_reports_its_declared_total() {
        let parsed = parse_report(
            "grouped-cxone.json",
            "fixtures/synthetic/grouped-cxone.json",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../fixtures/synthetic/grouped-cxone.json"
            ))
            .to_vec(),
        )
        .unwrap();
        let declared_total = parsed
            .report
            .metadata
            .declared_counts
            .iter()
            .find(|count| count.label == "declaredTotal")
            .expect("the report-level declaredTotal is retained as evidence");
        assert_eq!(declared_total.value, 99);
        assert_eq!(declared_total.locator, "/declaredTotal");
        // Only the report-level declaration disagrees with the parsed instances. Each section
        // total matches its own section, so it is not reported as a mismatch.
        assert_eq!(parsed.diagnostics.count_mismatches.len(), 1);
        assert_eq!(parsed.diagnostics.count_mismatches[0].declared, 99);
        assert_eq!(parsed.diagnostics.count_mismatches[0].parsed, 3);
    }

    #[test]
    fn unsupported_previews_mask_adjacent_secrets_without_panicking() {
        let parsed = parse_report(
            "unsupported.json",
            "/tmp/unsupported.json",
            br#"{"scanResults":[],"futureBranch":[{"password":"abc","token":"short","clientSecret":"ghp_abcdefghijklmnop","note":"kept"}]}"#
                .to_vec(),
        )
        .unwrap();
        assert_eq!(parsed.diagnostics.unsupported_records.len(), 1);
        let preview = &parsed.diagnostics.unsupported_records[0].preview;
        assert!(!preview.contains("abc"));
        assert!(!preview.contains("short"));
        assert!(!preview.contains("ghp_abcdefghijklmnop"));
        assert!(preview.contains("kept"));
    }

    #[test]
    fn long_previews_truncate_on_a_character_boundary() {
        let report = format!(
            r#"{{"scanResults":[],"futureBranch":[{{"note":"{}"}}]}}"#,
            "日".repeat(200)
        );
        let parsed = parse_report("edge.json", "/tmp/edge.json", report.into_bytes()).unwrap();
        let preview = &parsed.diagnostics.unsupported_records[0].preview;
        assert!(preview.ends_with('…'));
        assert!(preview.len() <= 320 + '…'.len_utf8());
    }

    #[test]
    fn null_and_unknown_states_are_diagnostic_not_panics() {
        let parsed = parse_report("edge.json", "/tmp/edge.json", br#"{"scanResults":null,"scaScanResults":null,"iacScanResults":null,"metadata":{"status":"mystery"}}"#.to_vec()).unwrap();
        assert_eq!(parsed.findings.len(), 0);
        assert!(parsed.diagnostics.warnings.len() >= 3);
    }

    #[test]
    fn checkmarx_summary_reports_are_accepted_without_findings() {
        let parsed = parse_report(
            "improved-project-report.json",
            "/tmp/improved-project-report.json",
            br#"{
                "reportType": "Improved Project Report",
                "header": {"tenantId": "4016c52f-1cd0-4307-99be-9713b71822b0", "timezone": "America/New_York"},
                "projectsOverview": {
                  "numberOfProjects": 1,
                  "projectsList": [{
                    "projectName": "deluxe-development/RaaS/raas-dashboardui",
                    "projectId": "28de8627-fe3a-445d-854b-216ff695abc9",
                    "severityDistribution": [{"level": "Critical", "value": 5}]
                  }]
                },
                "bySeverity": {
                  "totalResults": 18,
                  "severitiesBreakdown": [{"level": "Critical", "value": 1, "percentage": 5.56}]
                },
                "byScanner": {
                  "totalResults": 18,
                  "scannersDistribution": [{"scannerName": "SCA", "numberOfResults": 18, "percentage": 100}]
                }
            }"#
                .to_vec(),
        )
        .unwrap();
        assert_eq!(parsed.findings.len(), 0);
        assert_eq!(parsed.diagnostics.parsed_instances, 0);
        assert_eq!(parsed.report.metadata.project_id.as_deref(), Some("28de8627-fe3a-445d-854b-216ff695abc9"));
        assert_eq!(parsed.report.metadata.project_name.as_deref(), Some("deluxe-development/RaaS/raas-dashboardui"));
        assert!(parsed.diagnostics.format.contains("summary"));
    }

    #[test]
    fn result_oriented_unknown_engine_remains_visible() {
        let parsed = parse_report("results.json", "/tmp/results.json", br#"{"results":[{"id":"1","engine":"mystery","filePath":"src/a.ts","line":4,"severity":"not-a-value"}]}"#.to_vec()).unwrap();
        assert_eq!(parsed.findings.len(), 1);
        assert_eq!(parsed.findings[0].summary.severity, "Unknown");
        assert_eq!(
            parsed.findings[0].summary.category,
            FindingCategory::Unknown
        );
    }
}
