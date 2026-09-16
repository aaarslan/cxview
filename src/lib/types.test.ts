import { describe, expect, it } from "vitest";
import { profileLabel, reportLabel } from "./api";

describe("workspace labels", () => {
  it("keeps an imported snapshot distinct from a bound repository", () => {
    expect(reportLabel({ id: "r", sourceName: "fixture.json", sourcePath: "/tmp/fixture.json", sha256: "1234567890123456", adapterId: "cxone-grouped", adapterVersion: "1", importedAt: "now", findingCount: 2, scannerSections: ["scanResults"], metadata: { includedEngines: [], declaredCounts: [] } })).toContain("2 parsed instances");
    expect(profileLabel({ id: "p", name: "fixture", repositoryPath: "/tmp/repo", uiState: { search: "" } })).toBe("/tmp/repo");
  });
});
