use std::collections::{HashMap, HashSet};

use crate::models::*;

pub fn compare_reports(
    baseline: &ReportSummary,
    baseline_diag: &ImportDiagnostics,
    baseline_findings: &[FindingRecord],
    compared: &ReportSummary,
    compared_diag: &ImportDiagnostics,
    compared_findings: &[FindingRecord],
) -> ComparisonSummary {
    let (provenance_comparable, provenance_reason) =
        comparable_provenance(baseline, baseline_diag, compared, compared_diag);
    let compared_by_fingerprint: HashMap<&str, &FindingRecord> = compared_findings
        .iter()
        .map(|finding| (finding.summary.fingerprint.as_str(), finding))
        .collect();
    let baseline_fingerprints: HashSet<&str> = baseline_findings
        .iter()
        .map(|finding| finding.summary.fingerprint.as_str())
        .collect();
    let mut items = Vec::new();
    let mut still_observed = 0;
    let mut absent_under_comparable_scope = 0;
    let mut explicitly_reported_fixed = 0;
    let mut incomparable = 0;
    for finding in baseline_findings {
        let key = finding.summary.fingerprint.as_str();
        if let Some(current) = compared_by_fingerprint.get(key) {
            let current_status = current
                .summary
                .result_status
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if ["fixed", "resolved", "closed", "not exploitable"]
                .iter()
                .any(|status| current_status == *status)
            {
                explicitly_reported_fixed += 1;
                items.push(ComparisonItem { finding_id: finding.summary.id.clone(), classification: "explicitly reported fixed".to_owned(), reason: format!("The later export still contains the stable match and declares result status '{}'.", current.summary.result_status.as_deref().unwrap_or("unknown")) });
            } else {
                still_observed += 1;
                items.push(ComparisonItem {
                    finding_id: finding.summary.id.clone(),
                    classification: "still observed".to_owned(),
                    reason: "The versioned fingerprint is present in the later export.".to_owned(),
                });
            }
        } else if provenance_comparable {
            absent_under_comparable_scope += 1;
            items.push(ComparisonItem { finding_id: finding.summary.id.clone(), classification: "no longer observed under comparable scope".to_owned(), reason: "The finding is absent from an export whose project/branch/engine/filter/completeness provenance is comparable; this remains a scanner observation, not a local proof.".to_owned() });
        } else {
            incomparable += 1;
            items.push(ComparisonItem {
                finding_id: finding.summary.id.clone(),
                classification: "incomparable".to_owned(),
                reason: provenance_reason.clone(),
            });
        }
    }
    for finding in compared_findings {
        if !baseline_fingerprints.contains(finding.summary.fingerprint.as_str()) {
            items.push(ComparisonItem { finding_id: finding.summary.id.clone(), classification: "newly observed".to_owned(), reason: "No stable or fallback fingerprint match was found in the baseline export; review uncertain matches manually.".to_owned() });
        }
    }
    ComparisonSummary {
        baseline_report_id: baseline.id.clone(),
        compared_report_id: compared.id.clone(),
        provenance_comparable,
        provenance_reason,
        newly_observed: compared_findings
            .iter()
            .filter(|finding| !baseline_fingerprints.contains(finding.summary.fingerprint.as_str()))
            .count(),
        still_observed,
        absent_under_comparable_scope,
        explicitly_reported_fixed,
        incomparable,
        items,
    }
}

fn comparable_provenance(
    baseline: &ReportSummary,
    baseline_diag: &ImportDiagnostics,
    compared: &ReportSummary,
    compared_diag: &ImportDiagnostics,
) -> (bool, String) {
    if baseline.metadata.project_id.is_some()
        && compared.metadata.project_id.is_some()
        && baseline.metadata.project_id != compared.metadata.project_id
    {
        return (
            false,
            "Project IDs differ; absence cannot be interpreted as a remediation.".to_owned(),
        );
    }
    if baseline.metadata.branch.is_some()
        && compared.metadata.branch.is_some()
        && baseline.metadata.branch != compared.metadata.branch
    {
        return (
            false,
            "Branches differ; absence cannot be interpreted as a remediation.".to_owned(),
        );
    }
    if baseline.metadata.filters != compared.metadata.filters {
        return (false, "Export filters differ or are unknown on one side; omitted findings are not proof of remediation.".to_owned());
    }
    if baseline.metadata.scope != compared.metadata.scope {
        return (
            false,
            "Report scope differs or is unknown; comparison is conservative.".to_owned(),
        );
    }
    if !completeness_is_comparable(baseline.metadata.completeness.as_deref())
        || !completeness_is_comparable(compared.metadata.completeness.as_deref())
    {
        return (
            false,
            "At least one report has failed, partial, filtered, or unknown completeness."
                .to_owned(),
        );
    }
    if baseline_diag.adapter_id != compared_diag.adapter_id {
        return (
            false,
            "Parser adapter families differ; match coverage is not comparable.".to_owned(),
        );
    }
    if baseline_diag.parsed_by_engine.sast
        + baseline_diag.parsed_by_engine.sca
        + baseline_diag.parsed_by_engine.iac
        == 0
        && !baseline_findings_nonempty_hint(baseline_diag)
    {
        return (
            false,
            "Baseline has no parsed instances; comparison coverage is unknown.".to_owned(),
        );
    }
    (true, "Report provenance is comparable for the recorded fields; scanner absence remains separate from local task evidence.".to_owned())
}

fn completeness_is_comparable(value: Option<&str>) -> bool {
    let value = value.unwrap_or_default().to_ascii_lowercase();
    value.is_empty()
        || [
            "complete",
            "completed",
            "success",
            "succeeded",
            "successful",
        ]
        .iter()
        .any(|allowed| value == *allowed)
}

fn baseline_findings_nonempty_hint(diag: &ImportDiagnostics) -> bool {
    diag.parsed_instances > 0
}
