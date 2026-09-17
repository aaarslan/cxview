# CXView walkthrough

This is the shortest path through the product: take a synthetic scanner finding, bind it to code, make one deliberately small proposal, review the real diff, apply it, and run the repository’s own check.

The walkthrough is designed to teach the product’s most important rule:

> A scanner observation, a local code observation, a proposed change, a passing check, and a later scanner result are separate pieces of evidence.

The whole sequence below is also an automated test (`cargo test --manifest-path src-tauri/Cargo.toml --test fixture_walkthrough`). It runs the same fixture, the same anchors, and the same check against a disposable copy, so the steps and this document stay in step.

## Before you start

From the repository root, install the dependencies and launch the native app:

```sh
pnpm install
pnpm tauri dev
```

The Tauri CLI comes from `devDependencies` (`@tauri-apps/cli`). `pnpm install` is enough; no global `cargo install tauri-cli` is required.

The browser preview (`pnpm dev`) is useful for styling and component work, but this walkthrough needs Tauri’s native file selection, repository inspection, SQLite persistence, and Git commands. In the browser preview every native action reports that it is unavailable.

The demo uses:

- [`fixtures/synthetic/grouped-cxone.json`](../fixtures/synthetic/grouped-cxone.json) — one SAST, one SCA, and one IaC finding, plus deliberately visible diagnostics.
- [`fixtures/repositories/react-remediation`](../fixtures/repositories/react-remediation) — a tiny Node fixture with an inner Git repository and a test script.

The React fixture’s working tree is intentionally left with the reviewed fix applied. Its inner Git repository still records the vulnerable baseline, so every run of this walkthrough starts by restoring that baseline in a disposable copy.

### macOS/Linux

```sh
DEMO_ROOT="$(mktemp -d)"
cp -R fixtures/repositories/react-remediation/. "$DEMO_ROOT/react-remediation"
git -C "$DEMO_ROOT/react-remediation" restore --source=HEAD -- src/App.tsx
```

Bind the copied directory inside CXView. The original fixture in this repository is not changed.

### Windows PowerShell

```powershell
$DemoRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("cxview-demo-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $DemoRoot | Out-Null
Copy-Item -Recurse "fixtures/repositories/react-remediation" (Join-Path $DemoRoot "react-remediation")
git -C (Join-Path $DemoRoot "react-remediation") restore --source=HEAD -- "src/App.tsx"
```

## 1. Import the report

1. Click **Open report**.
2. Select `fixtures/synthetic/grouped-cxone.json`.
3. Read the diagnostics strip before selecting a finding.

The expected import is intentionally more interesting than a green “imported” toast:

| Signal | Expected value |
| --- | --- |
| Adapter | `cxone-grouped` v1.0.0 |
| Parsed findings | 1 SAST, 1 SCA, 1 IaC |
| Unsupported records | 1 retained for raw inspection |
| Declared count | 99, which does not match the 3 parsed instances |
| Report metadata | project ID `fixture-react`, project `react-remediation-fixture`, branch `main`, completeness `complete` |

The subbar above the workspace shows the snapshot hash, the parsed instance count, the adapter id and version, and the branch. The rest of the report metadata — project ID and name, scan ID, commit, timestamp, completeness, filters, and the declared counts — is retained with the snapshot and appears in an exported task bundle.

Expand **Evidence and compatibility notes**. The count mismatch is a feature, not a test failure: a report-declared total (`declaredTotal: 99`) and the number of individual records CXView actually parsed (3) are different facts. Totals declared inside a scanner section are compared only against that section, so they are not reported as report-wide mismatches.

## 2. Bind the repository

1. Click **Bind repository**.
2. Select the disposable `react-remediation` copy.
3. Leave **Scan prefix** and **Repository prefix** empty; the fixture already reports `src/App.tsx` relative to the repository root.
4. Click **Confirm repository**.

The binding grants CXView read access for the active workspace. It does not authorize a patch. If you bind a non-Git folder, CXView can still inspect and export proposals, but the reviewed-apply action remains unavailable.

## 3. Read one finding as evidence

Select the SAST finding whose title mentions **raw HTML**. You should see:

- the current `src/App.tsx` in the center pane;
- a **Current file differs** location state;
- three rows under **Reported flow**;
- scanner provenance and current-source provenance shown separately;
- the right-hand remediation panel with “What was reported”, “What current code shows”, and “Unknowns and drift”.

Two of these need explaining.

**Current file differs.** The fixture reports the snippet `dangerouslySetInnerHTML={{__html: value}}` while the file contains `dangerouslySetInnerHTML={{ __html: value }}`. CXView looks for the reported snippet in current bytes, does not find it, and says so instead of claiming a match. The drift is also listed under **Unknowns and drift**: “The reported snippet was not found in current bytes; the scan/source snapshot may have drifted.” The file itself is still located exactly, so the finding stays actionable.

**Three reported flow rows.** The first row is the finding’s own recorded location (`src/App.tsx:5`) and the next two are the flow nodes the report declares: a `source` at line 3 and a `sink` at line 5. The importer records the result’s own location as a node as well, so the sink line appears twice — once because the scanner located the finding there, and once because the scanner declared it as the sink.

Open **Inspect JSON** if you want to see the original retained record. The raw locator (`/scanResults/0/results/0`) is a path into the immutable imported snapshot; it is not a current-code claim.

This is the point where CXView asks you to establish whether the feature needs rich HTML. The fixture is intentionally text-only, but the product does not pretend that every raw-HTML use has the same repair.

## 4. Capture an investigation task

Click **Investigate**. This captures the finding’s current inputs as a task snapshot:

- report ID and finding identity;
- repository path, branch, and HEAD when Git is available;
- file hashes, byte lengths, line endings, and bounded source content;
- the evidence references needed to explain the proposal later.

No repository file changes. The task state moves to investigating, and the **Next safe action** line in the remediation panel tells you the next bounded step.

The offline React/DOM playbook is already visible in the same panel, under **Ask first**, **Preferred repair**, **Preserve + test**, and **Reject cosmetic fixes**. Click **Suggest fix** to capture the task and have the playbook called out in the message strip. It is contextual guidance, not an executable patch: it asks whether plain text is enough, what trust boundary exists, and what behavior and regression cases must be preserved.

## 5. Create a manual proposal

Click **Edit proposal buffer**. Fill the exact anchors below:

| Field | Value |
| --- | --- |
| Relative target path | `src/App.tsx` |
| Diagnosis | `This text-only preview does not need an HTML rendering sink.` |
| Exact old text anchor | `return <section dangerouslySetInnerHTML={{ __html: value }} />;` |
| Proposed new text | `return <section>{value}</section>;` |
| Behavior rationale | `Preserve the component's visible text output without interpreting the value as markup.` |
| Suggested regression test | `Run the fixture's actual test script and retain the result.` |

Leave **Create a new file** unchecked. Click **Create reviewed diff**.

The anchor has to be copied exactly as it appears in the file, including the spaces inside `{{ __html: value }}`. The native layer checks that the old text is present exactly once in the captured snapshot and that the target stays inside the selected repository. If the anchor is missing, ambiguous, or the current source has drifted, CXView stops and asks for a new review instead of guessing.

## 6. Review, then apply

The review dialog is the safety boundary. Before approving it:

1. Read the before/after CodeMirror diff.
2. Expand the canonical unified diff if you want the patch text.
3. Read the behavior rationale and unresolved risks.
4. Check the acknowledgement that you reviewed the complete diff.
5. Click **Apply reviewed patch**.

Applying is a separate approval from creating the proposal. Before writing, CXView rechecks the task’s base hashes and runs Git preflight. The patch is applied to the working tree without staging, committing, stashing, resetting, or cleaning it. The confirmation reads: “Reviewed patch applied to the working tree without staging, committing, stashing, or resetting.”

After success, the finding’s task badge reads **Patched · validation pending**, and the patch journal card states that scanner status is separate and that CXView will not mark the finding fixed until a comparable later export says so. No new scanner report has been imported.

## 7. Run a real local check

Click **Run checks**. CXView discovers candidates from the fixture’s actual `package.json`; it does not invent a command. The fixture offers two: `test` (`node --test`) and `typecheck` (`node --check tests/remediation.test.mjs`).

Choose the `test` candidate and read the command preview. The exact executable, arguments, working directory, possible writes, and network note are shown before approval. Approve one run.

The fixture’s test script reports two passing tests:

1. legitimate text behavior remains represented;
2. the raw HTML sink is gone.

The run is recorded against the task snapshot, with its status, exit code, duration, output, and note. A pass means the selected local check passed against the reviewed source. It is not a scanner verdict.

## 8. Try recovery and drift protection

Because this is a disposable copy, you can click **Undo this CXView patch** after the validation run. Undo is guarded by the patch journal’s post-hashes; if someone edits the file after CXView applies it, undo refuses to overwrite that later work. Applying the same reviewed proposal again after an undo is supported: the journal row is replaced rather than duplicated.

You can also prove the stale-input behavior:

1. Create a task and proposal.
2. Change the target file outside CXView.
3. Return to the review or apply action.

The operation fails with a message that the current bytes differ from the captured base hash and the patch is stale. That interruption is the expected safe result.

## 9. Optional proposal exchange

The workflow does not require an AI provider. You can use **Export task** to create a JSON handoff, edit a proposal with another tool, and import it for the same native validation and review path.

An imported proposal is bound to the exact task and snapshot: both `taskId` and `snapshotId` must equal the task it is imported into. Its shape is a `cxview-proposal-v1` document using camelCase keys:

```json
{
  "schemaVersion": "cxview-proposal-v1",
  "taskId": "<the task id from the exported bundle>",
  "snapshotId": "<the same task id>",
  "source": "imported",
  "provider": "your-tool-name",
  "diagnosis": "Explain the repair in context.",
  "assumptions": [],
  "edits": [
    {
      "path": "src/App.tsx",
      "oldText": "<exact unique text>",
      "newText": "<reviewed replacement>",
      "expectedAbsent": false
    }
  ],
  "behaviorPreservation": ["State what must remain true."],
  "suggestedTests": ["Name a real repository check."],
  "unresolvedQuestions": [],
  "evidenceRefs": ["/scanResults/0/results/0"]
}
```

`source` is replaced with `imported` on the way in, so the value you send does not change the provenance CXView records. An edit with `expectedAbsent: true` creates a new file and must carry exactly one edit with an empty `oldText`; it is rejected if the target already exists.

The optional Codex adapter follows the same contract. It is user initiated, uses the installed CLI’s read-only/structured-output controls when available, and returns a proposal for review. It cannot apply patches, run checks, or change scanner status. If the installed CLI does not advertise both controls, or `codex` is not on `PATH`, the adapter reports itself as disabled and the manual and external paths still work.

## 10. Explore the other fixtures

Try the other checked-in inputs to see how the importer communicates limits:

- [`result-oriented-cxone.json`](../fixtures/synthetic/result-oriented-cxone.json) exercises a top-level `results` array, unknown engine/severity values, and an unsupported record.
- [`null-sections-cxone.json`](../fixtures/synthetic/null-sections-cxone.json) proves that null scanner sections become warnings rather than crashes or fake zero-risk results.
- [`sca-multiple-versions`](../fixtures/repositories/sca-multiple-versions) exercises npm lockfile ownership with multiple installed versions.

The importer's job is not to make every input look supported. Its job is to preserve enough evidence for a human to understand what was and was not parsed.

## If something looks wrong

| Symptom | Meaning |
| --- | --- |
| “CXView native commands are unavailable…” | The browser preview is running; launch with `pnpm tauri dev`. |
| **Apply reviewed patch** is disabled | The proposal is not acknowledged in the review dialog, or the bound folder is not a Git worktree. A dirty or unsafe worktree is reported by the Git preflight when you apply. |
| A finding says **Ambiguous** or **Unavailable** | CXView could not prove the report path maps to one current file. It will not guess by basename. |
| A finding says **Current file differs** | The file was located exactly, but the scanner’s reported snippet is not in current bytes. The finding is still actionable; the drift is evidence. |
| **No check candidates** | The bound package root has no script whose name CXView recognises as a test, typecheck, lint, build, or verify script. |
| The Codex adapter is disabled | The CLI is missing, or its installed help does not advertise both structured output and a read-only control. Manual and external proposals still work. |
| “Count mismatch” appears | The report’s declared total and parsed individual instances disagree. Inspect the diagnostics before trusting coverage. |
| “No package.json was found inside the bound repository” | CXView will not run scripts discovered above the folder you bound, even if a parent directory has a manifest. |

For a contributor-facing overview of the architecture, boundaries, verification commands, and platform caveats, see the [root README](../README.md).
