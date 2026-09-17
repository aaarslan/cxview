# CXView walkthrough

This is the shortest path through the product: take a synthetic scanner finding, bind it to code, make one deliberately small proposal, review the real diff, apply it, and run the repository’s own check.

The walkthrough is designed to teach the product’s most important rule:

> A scanner observation, a local code observation, a proposed change, a passing check, and a later scanner result are separate pieces of evidence.

## Before you start

From the repository root, install the dependencies and launch the native app:

```sh
pnpm install
pnpm tauri dev
```

The browser preview (`pnpm dev`) is useful for styling and component work, but this walkthrough needs Tauri’s native file selection, repository inspection, SQLite persistence, and Git commands.

The demo uses:

- [`fixtures/synthetic/grouped-cxone.json`](../fixtures/synthetic/grouped-cxone.json) — one SAST, one SCA, and one IaC finding, plus deliberately visible diagnostics.
- [`fixtures/repositories/react-remediation`](../fixtures/repositories/react-remediation) — a tiny Node fixture with an inner Git repository and a test script.

The React fixture’s working tree is intentionally left with the reviewed fix after the native integration tests. To replay the apply flow from the vulnerable baseline, make a disposable copy first.

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
| Declared count | 99, which does not match the parsed instances |
| Report provenance | project `fixture-react`, branch `main`, complete export |

Expand **Evidence and compatibility notes**. The count mismatch is a feature, not a test failure: a report-declared total and the number of individual records CXView actually parsed are different facts.

## 2. Bind the repository

1. Click **Bind repository**.
2. Select the disposable `react-remediation` copy.
3. Leave **Scan prefix** and **Repository prefix** empty; the fixture already reports `src/App.tsx` relative to the repository root.
4. Click **Confirm repository**.

The binding grants CXView read access for the active workspace. It does not authorize a patch. If you bind a non-Git folder, CXView can still inspect and export proposals, but the reviewed-apply action remains unavailable.

## 3. Read one finding as evidence

Select the SAST finding whose title mentions **raw HTML**. You should see:

- the current `src/App.tsx` in the center pane;
- a **Matched** location state;
- a two-step scanner flow with a source and a sink;
- scanner provenance and current-source provenance shown separately;
- the right-hand remediation panel with “What was reported”, “What current code shows”, and “Unknowns and drift”.

Open **Inspect JSON** if you want to see the original retained record. The raw locator is a path into the immutable imported snapshot; it is not a current-code claim.

This is the point where CXView asks you to establish whether the feature needs rich HTML. The fixture is intentionally text-only, but the product does not pretend that every raw-HTML use has the same repair.

## 4. Capture an investigation task

Click **Investigate**. This captures the finding’s current inputs as a task snapshot:

- report ID and finding identity;
- repository path, branch, and HEAD when Git is available;
- file hashes, byte lengths, line endings, and bounded source content;
- the evidence references needed to explain the proposal later.

No repository file changes. The task state should move to an investigation state, and the panel should tell you the next safe action.

Click **Suggest fix** to read the offline React/DOM playbook. It is contextual guidance, not an executable patch. It should ask whether plain text is enough, what trust boundary exists, and what behavior/regression cases must be preserved.

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

The native layer checks that the old text is present exactly once in the captured snapshot and that the target stays inside the selected repository. If the anchor is missing, ambiguous, or the current source has drifted, CXView should stop and ask for a new review instead of guessing.

## 6. Review, then apply

The review dialog is the safety boundary. Before approving it:

1. Read the before/after CodeMirror diff.
2. Expand the canonical unified diff if you want the patch text.
3. Read the behavior rationale and unresolved risks.
4. Check the acknowledgement that you reviewed the complete diff.
5. Click **Apply reviewed patch**.

Applying is a separate approval from creating the proposal. Before writing, CXView rechecks the task’s base hashes and runs Git preflight. The patch is applied to the working tree without staging, committing, stashing, resetting, or cleaning it.

After success, the task should say that it is awaiting local validation or a rescan. It should not claim that Checkmarx is fixed; no new scanner report has been imported.

## 7. Run a real local check

Click **Run checks**. CXView discovers candidates from the fixture’s actual `package.json`; it does not invent a command.

Choose the `test` candidate and read the command preview. The exact executable, arguments, working directory, possible writes, and network note are shown before approval. Approve one run.

The fixture’s test script should report two passing tests:

1. legitimate text behavior remains represented;
2. the raw HTML sink is gone.

The result is stored with status, exit code, duration, output, and the task snapshot hash. A pass means the selected local check passed against the reviewed source. It is not a scanner verdict.

## 8. Try recovery and drift protection

Because this is a disposable copy, you can click **Undo this CXView patch** after the validation run. Undo is guarded by the patch journal’s post-hashes; if someone edits the file after CXView applies it, undo refuses to overwrite that later work.

You can also prove the stale-input behavior:

1. Create a task and proposal.
2. Change the target file outside CXView.
3. Return to the review or apply action.

The operation should fail with a re-review/stale-input message. That interruption is the expected safe result.

## 9. Optional proposal exchange

The workflow does not require an AI provider. You can use **Export task** to create a JSON handoff, edit a proposal with another tool, and import it for the same native validation and review path.

An imported proposal is bound to the exact task and snapshot. Its shape is a `cxview-proposal-v1` document using camelCase keys:

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

The optional Codex adapter follows the same contract. It is user initiated, uses the installed CLI’s read-only/structured-output controls when available, and returns a proposal for review. It cannot apply patches, run checks, or change scanner status.

## 10. Explore the other fixtures

Try the other checked-in inputs to see how the importer communicates limits:

- [`result-oriented-cxone.json`](../fixtures/synthetic/result-oriented-cxone.json) exercises a top-level `results` array, unknown engine/severity values, and an unsupported record.
- [`null-sections-cxone.json`](../fixtures/synthetic/null-sections-cxone.json) proves that null scanner sections become warnings rather than crashes or fake zero-risk results.
- [`sca-multiple-versions`](../fixtures/repositories/sca-multiple-versions) exercises npm lockfile ownership with multiple installed versions.

The importer's job is not to make every input look supported. Its job is to preserve enough evidence for a human to understand what was and was not parsed.

## If something looks wrong

| Symptom | Meaning |
| --- | --- |
| “Native commands are unavailable” | The browser preview is running; launch with `pnpm tauri dev`. |
| Apply is disabled | The selected folder is not Git-backed, the proposal is not reviewed, or the repository state is not safe to mutate. |
| A finding says **Ambiguous** or **Unavailable** | CXView could not prove the report path maps to one current file. It will not guess by basename. |
| No validation candidates | The bound package root has no supported test/typecheck/lint/build-like script. |
| Codex adapter is disabled | The CLI is missing or does not advertise both structured output and read-only controls. Manual and external proposals still work. |
| “Count mismatch” appears | The report’s declared total and parsed individual instances disagree. Inspect the diagnostics before trusting coverage. |

For a contributor-facing overview of the architecture, boundaries, verification commands, and platform caveats, see the [root README](../README.md).
