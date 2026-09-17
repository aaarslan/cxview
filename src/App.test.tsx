import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import App from "./App";

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
});
