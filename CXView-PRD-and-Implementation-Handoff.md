# CXView — Desktop Remediation Workbench

## Implementation-ready PRD and handoff

**Prepared:** September 16, 2026  
**Primary platform:** Windows 11, x64  
**Primary projects:** React, TypeScript, JavaScript, and Node.js  
**Product principle:** Turn an exported security finding into a reviewed, locally validated code change. Do not build a prettier report.

---

## 1. Your assignment

Build **CXView**, a lightweight, local-first desktop application that opens a Checkmarx One JSON report alongside a local repository and helps an engineer investigate findings, obtain contextual remediation suggestions, review actual proposed changes, apply approved changes, and validate the result.

The user works across approximately 14 repositories. Repeated Checkmarx authentication in separate editor windows is unacceptable. Exported files are the primary integration: CXView must never require a Checkmarx account, API key, IDE extension, or running Checkmarx service.

The defining workflow is:

**Open report + choose repository → inspect evidence and current code → prepare a remediation → review a real diff → apply approved changes → run selected checks → reconcile a later report.**

A severity dashboard, generic CWE advice, or a “copy prompt” button alone does not satisfy this assignment. Deliver the entire workflow. AI-generated patches are proposals, not automatically correct repairs.

Build working software, not just another plan. Inspect the implementation workspace first and preserve unrelated work. Resolve ordinary implementation decisions yourself. When evidence or a supported capability is missing, implement an explicit, useful limitation rather than fabricating data or a successful result.

## 2. Boundaries and release scope

### Required in the first usable release

Implement native file/folder selection and drag-and-drop, report compatibility diagnostics, persistent repository profiles, source navigation, finding-specific investigation, offline remediation guidance, one real optional AI proposal integration, manual/external proposal import, diff review, approved patch application with recovery, local validation, and conservative scan comparison.

Prioritize SAST remediation for JavaScript/TypeScript/React and SCA dependency investigation for Node projects. Import supported IaC findings and provide source navigation, report guidance, and remediation task export; do not build infrastructure deployment automation.

### Explicit exclusions

No CI configuration, GitHub Actions, build pipelines, pipeline recommendations, release automation, or CI badges. Local tests and local packaging are required.

No Electron, bundled Chromium, Docker deployment, browser-hosted server, background daemon, authentication system, cloud database, telemetry, analytics dashboard, embedded model, vector database, repository-wide embedding index, plugin marketplace, ticket integration, automatic commits, pushes, or pull requests.

Do not require SARIF conversion or PDF parsing. Explain that JSON with individual findings is needed when someone opens a PDF or a summary-only export. Do not silently treat either as a complete machine-readable findings report.

Keep macOS/Linux portability in the architecture, but Windows is the acceptance platform. Do not claim other platforms are tested unless they actually are.

## 3. Architecture: small by construction

Use **Tauri 2 + Rust + React + TypeScript + Vite**, with CSS Modules, a small reusable component layer, and **CodeMirror 6 plus its merge package** for source and diff views. Tauri uses the operating system webview; Windows packaging must account for the WebView2 prerequisite rather than claiming a universally self-contained tiny executable. [S1–S3]

Use **SQLite embedded in the Rust core** for profiles, normalized findings, local notes, task state, and validation history. Store immutable report files and bounded task/patch artifacts in the per-user application data directory, referenced by hash. Do not store application state inside the inspected repositories by default.

Use **Oxc’s Rust parser** for on-demand JS/JSX/TS/TSX syntax context: enclosing declarations, imports, calls, and source ranges. Pin a tested version and test conversion between UTF-8 parser offsets and editor line/column coordinates. Oxc is a parser, not a replacement for the repository’s type checker or a full security data-flow engine. [S4]

Keep parsing, filesystem access, Git operations, process execution, and persistence behind narrow Rust commands. The renderer receives paged finding summaries and selected code ranges, not complete copies of every report and repository. All repository access must be checked in the native layer, not merely hidden behind disabled UI controls.

Do not bundle Node, Git, Codex, or Claude Code. Opening reports, navigating source, reading guidance, and reviewing proposed changes must work without those installations. Detect prerequisites only when their features are requested. Use installed Git for Git-aware features and the repository’s installed toolchain for validation. Never bootstrap missing tools with an automatic package download.

One active workspace, one active proposal job, bounded caches, and lazy parsing are sufficient. Remember 14 profiles without maintaining 14 full indexes, editors, watchers, or language servers. Watch only relevant files in the active workspace and stop watching when it closes.

## 4. Import actual reports without losing evidence

### Compatibility must be demonstrated

Implement explicit, versioned adapters for fixture-backed CXone report layouts. Investigate the grouped web-report family containing sections such as `scanResults`, `scaScanResults`, and `iacScanResults`, and the result-oriented family containing a top-level `results` array. Add other variants only after inspecting real examples or trustworthy fixtures; do not assume any array named `vulnerabilities` has one universal meaning. [S5–S7]

The user has not supplied an actual export with this handoff. Do not claim their precise schema is verified. Search the implementation workspace for provided sanitized reports first; otherwise use attributable public fixtures and clearly labeled synthetic edge cases. Publish a small compatibility table identifying the exact shapes and scanner branches covered by tests.

DefectDojo is a reference, not a drop-in dependency or proof of complete coverage. At research time, its grouped-report SCA parsing method returned an empty list with a not-implemented comment, although other SCA paths existed. Inspect code and tests, preserve applicable licenses for anything reused, and do not copy that omission into CXView. [S7]

### Import diagnostics are part of the product

For each import, show detected format, scanner sections, parsed individual findings, unsupported records, malformed records, and available/missing evidence. Preserve unsupported items for raw inspection. Null sections, unknown enum values, missing locations, and an empty findings array must not crash the application.

Separate report-declared counts from actual parsed instance counts and explain mismatches. Query-group totals are not individual findings. Never discard a record silently, and never present a failed or partial import as “zero vulnerabilities.” Duplicate import of identical bytes should reuse the snapshot, not duplicate its findings.

Retain the original report, its SHA-256, adapter version, import diagnostics, and a JSON pointer or equivalent raw locator for each normalized instance. Render supplied descriptions and snippets as untrusted content; never execute embedded markup or load remote images.

### Preserve important distinctions

Keep scanner engine, original severity, original result status, and original triage state separate. Preserve original values even when normalizing display labels. Include Unknown and Informational rather than mapping missing values to Low.

Capture project/scan IDs, branch, commit, timestamps, included engines, filters, scan completeness, and report scope when present. Unknown metadata remains unknown. Checkmarx reports can be filtered by severity, scanner, result status, and triage state; absence from an export is therefore not proof of remediation. [S5]

For SAST, retain every available path, ordered node, location, source/sink annotation, snippet, query identifier, CWE, recommendation, and evidence link. Preserve path multiplicity. Do not infer that the first available node is necessarily a confirmed source unless the format establishes that meaning.

For SCA, retain advisory aliases, ecosystem, package identity/version, affected and fixed ranges if supplied, dependency paths, and available reachability evidence. For IaC, retain resource location, rule, expected/actual values, and provider/context. Missing fields must never be invented.

## 5. Bind the report to the right code

After import, select a repository folder. Suggest a saved profile only when there is convincing project/path evidence, and require confirmation for an ambiguous association. Persist approved path mappings.

Handle Windows and POSIX separators, scan-container prefixes, nested monorepo roots, spaces, Unicode, and case-sensitive differences. Suggest prefix mappings, but never select a file merely because its basename matches. Several `index.ts` files are a normal ambiguity, not an excuse to guess.

Distinguish these states in the UI: **matched; relocated with evidence; ambiguous; unavailable; current file differs from scanned evidence**. Display scanned snippets and current source as separately labeled evidence. A report with endpoint locations but no intermediate nodes must say that the full reported flow is unavailable; still show endpoints and current code.

Use exact path and location first, then unique snippet/symbol evidence for suggested relocation. Show the reason for a proposed mapping. An approximate mapping is not authorization to edit that location.

Read Git HEAD, branch, staged/unstaged state, and relevant file hashes. If the scan commit is available locally, allow read-only inspection of historical source without changing the checkout. Do not infer a commit from its date or fetch remote history automatically.

Before creating a proposal, capture a manifest of the precise current inputs. Before applying it, recheck every target. A changed branch, modified target, stale report location, or conflicting editor change must produce a clear rebase/review path rather than a blind write.

Support ordinary folders in inspection/proposal mode without Git. Explain the missing prerequisite when Git-backed patch application is requested. Treat WSL/UNC/network roots explicitly: do not mix Windows and Linux commands or silently execute against a translated path. In the first release, unsupported execution locations may remain read-only with an export fallback.

## 6. Primary screen: the remediation workspace

Open directly into the last workspace or an **Open report + Select repository** screen. No dashboard landing page.

Use a resizable three-pane layout with a collapsible validation drawer:

- **Work queue:** searchable findings or candidate remediation groups. Each item identifies the rule, affected file/package, local task state, and evidence readiness; severity is a compact badge.
- **Evidence and code:** current source, reported source/sink/flow steps, jump-to-location, related files, and scanned-versus-current comparison.
- **Remediation:** explanation, knowns/unknowns, applicable guidance, behavior constraints, proposed changes, and review actions.

Primary actions are **Investigate**, **Suggest fix**, **Review changes**, **Apply reviewed patch**, and **Run checks**. An unavailable action explains the exact missing prerequisite. No decorative buttons, fake streaming, or placeholder success states.

Keep severity/engine/CWE/file/package/status filters available without letting them dominate. Include useful saved views: Needs investigation, Ready to review, Patched awaiting validation, and Awaiting rescan. Show active filters clearly; never hide lower-severity findings without indicating it.

Remember selected finding, scroll position, pane sizes, filters, notes, and unfinished tasks per profile. Include keyboard-first search, command palette, next/previous finding and flow-step navigation, accessible focus management, light/dark themes, scalable code text, and color-independent status labels. Keep core actions usable at 1280×800 and at common Windows display scaling levels.

Code is read-only by default. Allow editing a **proposal buffer**, not an untracked live-file editor. Recompute the diff and invalidate prior validation when the proposal changes. Open-in-editor is an optional convenience and must not require any security extension.

## 7. Investigation must explain this finding

Each finding gets an evidence-grounded investigation containing:

**What was reported:** exact scanner claim and original evidence.

**What the current code shows:** relevant declarations, imports, calls, framework context, and file/range references, each tied to a snapshot.

**What is not established:** missing nodes, external service behavior, sanitizer assumptions, unsupported syntax, unresolved dependencies, or scan/source drift.

**What a repair must preserve:** legitimate rendering, valid redirects, query behavior, API contract, authentication, tenant boundaries, or package compatibility.

Keep “reported by Checkmarx,” “observed locally,” “user supplied,” and “AI hypothesis” visibly distinct. A TypeScript type, a regex match, a suspicious API name, or an LLM confidence score is not proof of exploitability or safety.

Start source context with the implicated function/component and imports; expand to relevant local callers, configuration, or tests on demand. Avoid reading the entire repository or every installed package. Respect monorepo package boundaries and path aliases; flag unresolved mappings rather than inventing call chains.

Support local notes and a false-positive investigation draft, with code evidence and reasoning. Label these as local assessments. Do not alter Checkmarx state or represent an AI suggestion as a confirmed false positive.

## 8. Useful suggestions without AI

Ship a small, original, versioned library of actionable remediation playbooks. Each entry needs applicability conditions, concrete investigation questions, preferred repair approaches, behavior-preservation requirements, regression-test ideas, contraindications, authoritative references, and a last-reviewed date.

Prioritize React/DOM XSS, unsafe redirects, SQL injection, path traversal, command injection, SSRF, hardcoded secrets, and vulnerable dependencies. Entries should select relevant questions from the scanner rule and observed code context, not produce the same generic paragraph for every CWE.

For example, do **not** automatically replace every `dangerouslySetInnerHTML` use with JSX text. First establish whether the feature requires rich HTML. A text-only feature and an intentionally rich-text renderer require different repairs, and a successful patch must preserve intended behavior. React and OWASP document the risks around raw HTML and context-sensitive XSS prevention; write the guidance against those sources rather than inventing a universal sanitizer. [S8–S9]

Guidance must reject cosmetic fixes: suppressing the rule, renaming variables, hiding a sink behind a wrapper, adding broad casts, swallowing errors, or weakening authentication. A secrets finding needs a rotation/revocation follow-up where applicable; deleting a literal alone is not evidence the exposed credential is safe.

Do not claim a playbook is an executable repair. Offline users can inspect evidence, edit a proposal, import an externally produced patch, apply it through review, and run local checks. General repository-specific code generation requires the optional model integration or an external coding tool.

## 9. Optional AI: one thin integration, not another agent platform

Implement one genuine **Codex CLI proposal adapter** using the user’s installed CLI. Its documented non-interactive interface supports structured output, a read-only sandbox, and reuse of saved CLI authentication. Verify behavior against the installed version rather than hard-coding undocumented flags. No Checkmarx authentication is involved, but provider sessions can still expire. [S10]

The adapter’s job is to return a proposal, not modify the real repository. Use an app-owned, bounded context snapshot and a documented proposal-only configuration. Keep write access to the original repository out of the proposal process. Do not enable unrestricted access, blanket approval bypasses, additional directories, unrelated connectors, or autonomous subagents. Do not modify the user’s global CLI configuration or copy/extract its credentials.

Test the actual permissions and tool behavior. A working directory is not a security sandbox, and a read-only filesystem mode is not automatically a restriction on reads or network access. Disclose the provider’s real capabilities and data destination. Where the required isolation cannot be enforced, disable integrated generation with a precise diagnostic and retain external proposal exchange; do not advertise guarantees the adapter cannot deliver.

Provide a payload preview showing selected files/snippets, finding evidence, omissions, and destination. Cloud processing must be an explicit user-enabled feature. Do not describe a locally launched cloud CLI as local inference. Reuse a per-profile provider selection and appropriate consent rather than introducing per-window logins.

Bound context size and execution time, allow cancellation, limit concurrent jobs to one initially, and show real progress/error/output events. Surface authentication expiry, rate limits, missing CLI, unsupported version, invalid output, and cancellation distinctly. Never auto-retry an unbounded series of paid requests.

The output contract must include task/snapshot IDs, evidence references, diagnosis, assumptions, proposed text edits, behavior-preservation rationale, suggested tests, and unresolved questions. It must not accept claimed test success as validation evidence.

For reliable edits, include each target’s app-generated base hash and either exact old/new text or a supported patch representation. CXView must independently validate paths, base contents, unique replacement anchors, and edit overlap before producing a canonical diff. Support creating regression-test files with an explicit expected-absent precondition. Reject malformed or ambiguous output.

Also ship provider-neutral **Export remediation task**, **Copy investigation prompt**, and **Import proposal/patch**. This lets users work with Claude Code, Codex, or another approved tool without an integrated adapter. The exported task includes the evidence, current-code manifest, scope, behavior constraints, output schema, and validation plan. Return imports must bind to that task/snapshot; they must not apply themselves. A second native provider adapter is not necessary for the first release.

## 10. SCA: identify the dependency change that matters

A package finding needs an answer to “Why is this installed, and which change should I review?”

Detect the relevant package/workspace root, package manager, declared dependency, lockfile, and locally resolved version where available. Separate the version in the scan from the version currently present. Preserve multiple installed versions and multiple dependency paths.

Implement fixture-tested static support for npm `package-lock.json` versions 2/3 first. Detect pnpm, Yarn, and Bun and show declared-package/report evidence even when a particular lockfile format is unsupported. Do not claim a complete graph from a partial parser. Provide reviewed package-manager inspection commands for installed, supported versions to close those gaps; npm’s `explain` command is one documented source of dependency-chain evidence. [S11]

Show direct versus transitive ownership, affected paths, production/development/optional/peer context when established, and upgrade candidates supported by the report or explicitly retrieved advisory metadata. Unknown reachability remains unknown; “dev dependency” alone is not a reason to discard a finding.

Prefer a compatible update to the responsible direct dependency over forcing arbitrary transitive versions. Explain peer, engine, semver, workspace, and behavior compatibility concerns. Do not invent fixed versions or assume the newest version is a safe replacement.

Dependency changes use the detected package manager to regenerate lockfiles after explicit approval. Do not hand-edit integrity hashes or let an LLM invent a lockfile. Generate the candidate in an on-demand disposable staging workspace containing the necessary workspace manifests and approved settings, without duplicating `node_modules` or copying secrets. Preview the command and any network/install requirements; review the actual generated manifest/lockfile diff before applying it to the real repository through the normal patch path. If that package-manager/version cannot produce a trustworthy staged proposal, offer the command as an explicit external workflow rather than silently mutating the original repository. Record generated files and clean up temporary artifacts. Do not run a force-upgrade/fix command automatically.

Keep network lookup optional and separate from report import. If later adding advisory lookup, preview what is sent, avoid disclosing internal package names or the whole dependency graph by default, and cache results with source and retrieval time. Do not turn CXView into a competing vulnerability scanner.

## 11. Review, apply, recover

All proposal sources use the same patch-review and application path: AI, imported patch, deterministic transformation, or manual proposal editing.

Show a real unified or side-by-side diff, touched files, associated findings, new tests, behavior rationale, and unresolved risks. Review complete coherent patches initially; arbitrary hunk selection must not split required source/test or manifest/lockfile changes and then inherit the old validation status.

Before application, verify task/repository identity, every base hash, branch context, path scope, file encoding, and absence preconditions for new files. Reject `.git` changes, traversal paths, symlink/junction escapes, binary edits, and unsupported operations. Preserve line endings, Unicode, and staged changes.

Use installed Git for checked patch application. `git apply --check` establishes applicability, not correctness. Apply to the working tree without implicitly staging, committing, or invoking conflict-resolution modes that alter the index. Do not use permissive options to force stale patches through. [S12]

One deliberate approval authorizes one displayed patch transaction. Opening a report never authorizes writes. Allow unrelated dirty files; do not require a clean repository merely for convenience. Never auto-stash, reset, clean, discard edits, or switch branches.

Create a bounded recovery journal with original bytes and post-application hashes for touched files. Prevent concurrent CXView writes to the same repository. Preflight the whole patch, handle interrupted application, and never silently leave an unexplained partial change. Multi-file recovery must be tested, not described as magically atomic.

Provide **Undo this CXView patch**, not “reset repository.” Undo only when post-image checks prove it will not destroy subsequent work. Otherwise show a conflict and export the inverse change for manual reconciliation. Application errors must preserve enough evidence to recover.

Changes made in an external editor should refresh source views and invalidate affected proposals. Permit linking an externally made change to a remediation task, but do not claim CXView authored unrelated changes.

## 12. Local validation with honest evidence

Discover candidate commands from the relevant workspace’s actual manifests/configuration. Support targeted tests, type checking, lint, and build where available. Never invent `test` or `typecheck` scripts or substitute a successful no-op.

Preview executable, arguments, working directory, relevant environment overrides, expected writes, and network implications. Repository scripts are executable code and may run pre/post hooks; trust and consent must cover the actual command chain, not just the visible script name. Reconfirm saved approvals when the relevant script definition changes. Do not pass model-provider secrets to test processes. [S13]

Use structured native process launching, not concatenated shell strings. Handle Windows executable/wrapper discovery and quoting explicitly. Stream bounded stdout/stderr, record exit codes and duration, support timeouts/cancellation, and terminate child process trees reliably.

Offer a baseline run before changes and a post-change run. Distinguish pre-existing failures, new failures, unknown baseline, missing dependencies, skipped checks, canceled checks, and passes. Do not install dependencies or change toolchains without approval.

Persist each validation run against the exact proposal and source snapshot, including relevant configuration and lockfile hashes. Further edits invalidate the applicability of previous results. Include negative/security regression cases and legitimate-input behavior tests in the proposal; never equate passing tests with a scanner-confirmed fix.

Track separate axes:

**Local task:** investigating → proposal ready → applied → locally validated → awaiting rescan, with explicit blocked/failed alternatives.

**Scanner observation:** still reported; explicitly reported fixed; absent from a comparable export; comparison inconclusive.

Neither axis silently overwrites the other.

## 13. Fix related findings without false deduplication

Preserve every imported finding instance. Offer **candidate remediation groups** when evidence suggests several findings share a repair: a shared sink/helper, the same rule and code boundary, or the same vulnerable package instance and upgrade path.

Show why each member is grouped and let the user split or exclude it. “Twelve findings might be addressed by this helper change” is a hypothesis, not a count of confirmed fixes. A shared CWE, line, or package name alone is insufficient.

Use scanner identifiers within their project/engine scope when available. A fallback fingerprint should include rule/query, normalized locations, relevant path or package identity, and a versioned algorithm. Treat collisions and uncertain cross-scan matches as review cases.

Allow batch investigation/export and coherent shared patches, but do not ship a “fix every High” button. Recheck overlapping files between tasks and invalidate or rebase competing proposals rather than applying them sequentially against stale inputs.

## 14. Reimport, comparison, and persistence

Import later reports as immutable snapshots linked to the appropriate profile. Compare only with visible provenance: project, branch, commit when available, engines, export filters, scan scope/completeness, and parser coverage.

Classify matches conservatively as newly observed, still observed, no longer observed under comparable scope, explicitly reported fixed, or incomparable. An absent finding in a filtered, partial, failed, differently scoped, or unknown-completeness report is not a confirmed repair. Keep local patch/test evidence available even if scanner identifiers change.

Persist notes, triage rationale, proposal revisions, patch history, and validation results across restarts. Use actual SQLite transactions and migrations. Do not retain a full copy of every repository; store only the bounded content needed for reproducible active tasks and recovery.

Provide storage usage, retention controls, deletion of a workspace’s local data, and export of a compact remediation summary/patch/task bundle. Explain that reports, snapshots, and patches can contain proprietary code or secrets. Mask sensitive values in normal views and export previews, avoid raw findings in diagnostic logs, and make copying/revealing source intentional. Do not claim forensic secure deletion or encryption the app has not implemented.

## 15. Safety without friction

Bundle UI assets locally. Use a restrictive content security policy and narrow Tauri capabilities. Deny arbitrary filesystem, shell, and network capabilities to report content. Enforce repository-root containment natively, including Windows reparse points and path normalization edge cases. A lexical prefix comparison alone is not sufficient. [S14]

Separate **read permission**, **proposal-provider consent**, **patch approval**, and **command approval**. Reading the folder the user selected should not cause repeated prompts. Applying one reviewed patch should not require a password or approval for every line. Permissions must remain scoped and revocable.

Treat report strings, repository comments, model output, and imported task bundles as untrusted data. They cannot authorize additional file access, network requests, commands, or writes. Prompt text does not replace the native enforcement layer.

No automatic outbound requests in the core workflow. Explicitly enabled model execution, package operations, external links, and user-approved commands have separately disclosed network behavior. Do not claim control over an external tool’s traffic merely because CXView’s renderer has no network permission.

## 16. Performance and packaging acceptance targets

These are engineering targets to measure, not pre-existing product claims:

| Area | Initial target |
|---|---|
| Release installer payload | At most 40 MiB, excluding an independently installed WebView2 runtime and external tools |
| Cold launch | Usable initial window within 2 seconds on a recorded Windows 11 SSD reference machine |
| Idle memory | At most 200 MiB across CXView and its attributable webview child processes with no report open |
| Representative report | Import 10,000 individual findings / approximately 50 MiB within 5 seconds, with a responsive and cancellable UI |
| Loaded memory | Target at most 400 MiB on that representative fixture |
| Finding navigation/filtering | Under 100 ms for cached results on the representative fixture |
| Inactive profiles | Metadata only; no persistent per-repository process or full source index |

Use background native tasks, bounded queues, pagination/virtualization, lazy code/AST loading, and explicit size/depth limits. Record both fixture bytes and finding/node counts; “10,000 findings” can conceal very different path sizes. Test oversize/malformed inputs and cancellation. Do not hide expensive work behind an unresponsive spinner.

Package locally for Windows with a normal per-user installer. Explain the WebView2 prerequisite and behavior when it is missing; an offline runtime bundle is larger and should not be misrepresented as the tiny default. No administrator requirement for normal usage. Clearly label signing status rather than inventing a trusted signature. [S2]

## 17. Required acceptance scenarios

**A. Real-format import:** A supported grouped CXone fixture imports all represented SAST instances and preserves all supplied paths. Supported SCA/IaC branches import too. Unsupported branches and count mismatches are visible. Importing null sections or unknown states does not crash or erase prior data.

**B. Code matching:** A Windows monorepo with repeated filenames maps correctly after one explicit prefix mapping. A missing file, ambiguous basename, outdated line, and unavailable scan commit produce understandable states. No guess becomes an edit silently.

**C. Actual remediation:** In a disposable React/TypeScript fixture, a finding opens beside relevant code, a real proposal is produced or imported, the user reviews its diff, applies it, and runs a real regression test. The result preserves legitimate behavior and is labeled awaiting rescan rather than scanner-confirmed fixed.

**D. Optional AI integration:** A real installed Codex invocation returns a schema-validated proposal on a benign synthetic case without writing the actual repository. Missing authentication, unsupported permissions/version, invalid output, cancellation, and provider errors are handled. Mock adapter tests do not substitute for this live smoke test; when credentials are unavailable, explicitly mark it unverified.

**E. Dirty work and stale proposals:** Unrelated staged/unstaged changes survive application. A target changed after proposal generation blocks application. Competing patches invalidate one another appropriately. Undo preserves subsequent edits or stops with a clear conflict.

**F. Dependency finding:** A fixture with multiple versions of a transitive npm dependency shows the correct paths and responsible direct dependency. Any proposed version comes from supplied advisory evidence. A reviewed package-manager operation produces the lockfile change; CXView does not fabricate it.

**G. Validation honesty:** A failing, missing, canceled, or skipped check is never a pass. Changed source/configuration invalidates old evidence. Script execution requires the appropriate approval and does not inherit provider secrets.

**H. Comparison honesty:** A later export with different severity/state filters cannot mark omitted findings fixed. Stable identifiers, fallback matches, and uncertain matches are visibly distinct. Local notes and patch history survive restart and reimport.

**I. Hostile input and recovery:** Test report markup, prompt-injection text, path traversal, Windows junction/symlink escape, command-injection strings, malformed/oversized JSON, unsupported patch operations, and an interrupted multi-file patch. No unapproved action or silent data loss results.

**J. Desktop quality:** Test the native packaged Windows app, not only a browser preview. Demonstrate keyboard navigation, focus after dialogs, scaling, drag-and-drop, cancellation, process cleanup, persistence, and measured performance. Do not claim measurements from an untested operating system.

Implement focused Rust and TypeScript unit tests, parser fixture tests, patch/process integration tests in disposable repositories, and UI tests for the key workflow. Run all verification locally. No CI files.

## 18. Implementation sequence and final delivery

Build in end-to-end slices, but do not stop at the first slice and call it the product.

**First:** verify report fixtures, establish the normalized evidence model, and complete report → repository → finding → code navigation in the native shell.

**Second:** complete offline investigation, task export/proposal import, real diff review, checked application, recovery, and validation. Prove one actual repair end to end before adding more presentation polish.

**Third:** integrate the real optional proposal adapter, SCA dependency context, shared-remediation groups, persistence, and conservative comparison. Keep provider/version failures explicit.

**Finally:** exercise edge cases, test the packaged Windows experience, measure budgets, and simplify unnecessary dependencies/UI.

Keep modules focused: import adapters, repository/evidence access, remediation tasks, proposal providers, patch transactions, validation, persistence, and UI. Do not create a general-purpose agent framework or speculative abstraction layer.

Deliver working source, local build/run/test commands, the locally built Windows artifact when the environment supports it, representative non-sensitive fixtures, a short README with compatibility/prerequisite/privacy details, and a concise verification report listing actual checks and measurements. Record upstream source/license attribution where relevant.

If a capability cannot be completed or verified in the available environment, say exactly which one, what works instead, and what evidence is missing. Never fabricate a live test, successful repair, supported schema, or performance result.

The final demonstration must follow this sequence:

**Import a supported CXone JSON report → bind its repo → investigate a real fixture finding → obtain a candidate repair → review and apply its diff → run a meaningful check → reopen the app and retain the evidence.**

The success criterion is not “the dashboard renders.” It is **“an engineer can make a justified, reviewable code change without fighting Checkmarx authentication or deciphering raw JSON.”**

---

## Research references

These are implementation references, not a claim that the user’s exact report or environment has been validated. Documentation and moving repository branches can change; record the versions/commits actually used during implementation.

- **S1 — Tauri architecture:** https://v2.tauri.app/concept/architecture/
- **S2 — Tauri Windows distribution and WebView2 options:** https://v2.tauri.app/distribute/windows-installer/
- **S3 — CodeMirror merge package:** https://github.com/codemirror/merge
- **S4 — Oxc parser:** https://oxc.rs/docs/guide/usage/parser.html and https://docs.rs/oxc_parser/latest/oxc_parser/
- **S5 — Checkmarx One scan reports, filters, and report sections:** https://docs.checkmarx.com/en/34965-182434-checkmarx-one-reporting.html
- **S6 — DefectDojo Checkmarx One parser documentation:** https://docs.defectdojo.com/supported_tools/parsers/file/checkmarx_one/
- **S7 — DefectDojo Checkmarx One parser implementation:** https://github.com/DefectDojo/django-DefectDojo/blob/master/dojo/tools/checkmarx_one/parser.py
- **S8 — React raw HTML documentation:** https://react.dev/reference/react-dom/components/common#dangerously-setting-the-inner-html
- **S9 — OWASP XSS prevention:** https://cheatsheetseries.owasp.org/cheatsheets/Cross_Site_Scripting_Prevention_Cheat_Sheet.html
- **S10 — Codex non-interactive execution:** https://developers.openai.com/codex/noninteractive/
- **S11 — npm dependency-chain inspection:** https://docs.npmjs.com/cli/v11/commands/npm-explain/
- **S12 — Git patch application:** https://git-scm.com/docs/git-apply
- **S13 — npm script and lifecycle behavior:** https://docs.npmjs.com/cli/v11/using-npm/scripts/
- **S14 — Tauri capability model:** https://v2.tauri.app/security/capabilities/
