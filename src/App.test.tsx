import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import App, { overlayFlags } from "./App";

const noOverlays = {
  showBind: false,
  modal: null,
  validationOpen: false,
  storageOpen: false,
} as const;

describe("desktop workbench entry", () => {
  let container: HTMLDivElement | undefined;
  let root: ReturnType<typeof createRoot> | undefined;

  beforeAll(() => { (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true; });

  afterEach(() => {
    act(() => { root?.unmount(); });
    container?.remove();
    root = undefined;
    container = undefined;
  });

  it("opens on the report-and-repository workflow instead of a dashboard", async () => {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    await act(async () => { root?.render(<App />); await new Promise((resolve) => setTimeout(resolve, 25)); });
    expect(container.textContent).toContain("Evidence before edits.");
    expect(container.textContent).toContain("Open report");
    expect(container.textContent).toContain("Browser preview");
    expect(container.textContent).not.toContain("native commands are unavailable");
    expect(container.textContent).not.toContain("severity dashboard");
  });

  it("never blocks the window for a dialog that cannot be rendered", () => {
    // Dropping a report clears the task, investigation, and command candidate behind an open
    // dialog. A stale blocking flag would then inert the whole window with nothing mounted.
    expect(overlayFlags({ ...noOverlays, modal: "proposal" }).blocking).toBe(false);
    expect(overlayFlags({ ...noOverlays, modal: "command" }).blocking).toBe(false);
    expect(overlayFlags({ ...noOverlays, showBind: true }).blocking).toBe(false);
    expect(overlayFlags({ ...noOverlays, storageOpen: true }).blocking).toBe(false);
  });

  it("blocks the window only while an overlay is actually mounted", () => {
    const report = { id: "report-1", sourceName: "grouped-cxone.json" } as never;
    const task = { id: "task-1" } as never;
    const investigation = { finding: { id: "finding-1" } } as never;
    expect(
      overlayFlags({ ...noOverlays, showBind: true, report }).blocking,
    ).toBe(true);
    expect(
      overlayFlags({ ...noOverlays, modal: "proposal", task, investigation }).blocking,
    ).toBe(true);
    expect(overlayFlags({ ...noOverlays, modal: "provider" }).blocking).toBe(true);
    expect(overlayFlags({ ...noOverlays, validationOpen: true }).blocking).toBe(true);
  });
});
