import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { EditorView } from "@codemirror/view";
import { CodeSurface, DiffSurface } from "./CodeSurface";
import type { DiffFile } from "../lib/types";

describe("CodeSurface", () => {
  let container: HTMLDivElement | undefined;
  let root: ReturnType<typeof createRoot> | undefined;

  beforeAll(() => {
    (globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;
  });

  afterEach(() => {
    act(() => {
      root?.unmount();
    });
    container?.remove();
    root = undefined;
    container = undefined;
  });

  function mount(node: React.ReactNode): void {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    act(() => {
      root?.render(node);
    });
  }

  it("keeps one read-only editor while updating source, font size, and jump inputs", () => {
    mount(<CodeSurface content={"const first = 1;\nconst second = 2;"} filePath="src/example.ts" focusLine={2} />);

    const host = container?.querySelector('[role="region"]') as HTMLDivElement;
    const editor = host.querySelector(".cm-editor") as HTMLElement;
    const content = host.querySelector(".cm-content") as HTMLElement;
    const view = EditorView.findFromDOM(editor);

    expect(host.getAttribute("aria-label")).toBe("Read-only source for src/example.ts");
    expect(content.getAttribute("role")).toBe("textbox");
    expect(content.getAttribute("aria-label")).toBe("Read-only source for src/example.ts");
    expect(content.getAttribute("aria-readonly")).toBe("true");
    expect(content.getAttribute("contenteditable")).toBe("false");
    expect(view?.state.doc.toString()).toContain("const first = 1;");
    expect(host.style.getPropertyValue("--cx-code-font-size")).toBe("13px");

    act(() => {
      root?.render(<CodeSurface content={"const first = 10;\nconst second = 20;"} filePath="src/example.js" fontSize={17} focusLine={1} focusToken={1} />);
    });

    const nextEditor = host.querySelector(".cm-editor") as HTMLElement;
    const nextContent = host.querySelector(".cm-content") as HTMLElement;
    const nextView = EditorView.findFromDOM(nextEditor);
    expect(nextEditor).toBe(editor);
    expect(nextView).toBe(view);
    expect(nextView?.state.doc.toString()).toBe("const first = 10;\nconst second = 20;");
    expect(host.getAttribute("aria-label")).toBe("Read-only source for src/example.js");
    expect(nextContent.getAttribute("aria-label")).toBe("Read-only source for src/example.js");
    expect(host.style.getPropertyValue("--cx-code-font-size")).toBe("17px");
  });

  it("keeps both diff editors stable across file identity, content, path, and font changes", () => {
    const first: DiffFile = { path: "src/example.ts", before: "const first = 1;", after: "const first = 2;" };
    mount(<DiffSurface file={first} />);

    const host = container?.querySelector('[role="region"]') as HTMLDivElement;
    const editors = Array.from(host.querySelectorAll(".cm-editor")) as HTMLElement[];
    const views = editors.map((editor) => EditorView.findFromDOM(editor));
    expect(editors).toHaveLength(2);
    expect(views.every(Boolean)).toBe(true);
    expect(host.getAttribute("aria-label")).toBe("Reviewed before and after diff for src/example.ts");
    expect(host.querySelectorAll('[aria-readonly="true"]')).toHaveLength(2);

    act(() => {
      root?.render(<DiffSurface file={{ ...first }} fontSize={18} />);
    });
    expect(Array.from(host.querySelectorAll(".cm-editor"))).toEqual(editors);
    expect(host.style.getPropertyValue("--cx-code-font-size")).toBe("18px");

    act(() => {
      root?.render(<DiffSurface file={{ path: "src/example.js", before: "const first = 10;", after: "const first = 20;" }} fontSize={18} />);
    });
    const nextEditors = Array.from(host.querySelectorAll(".cm-editor")) as HTMLElement[];
    const nextViews = nextEditors.map((editor) => EditorView.findFromDOM(editor));
    expect(nextEditors).toEqual(editors);
    expect(nextViews).toEqual(views);
    expect(nextViews[0]?.state.doc.toString()).toBe("const first = 10;");
    expect(nextViews[1]?.state.doc.toString()).toBe("const first = 20;");
    expect(host.getAttribute("aria-label")).toBe("Reviewed before and after diff for src/example.js");
    expect(nextEditors[0].querySelector(".cm-content")?.getAttribute("aria-label")).toBe("Before source for src/example.js");
    expect(nextEditors[1].querySelector(".cm-content")?.getAttribute("aria-label")).toBe("After source for src/example.js");
  });
});
