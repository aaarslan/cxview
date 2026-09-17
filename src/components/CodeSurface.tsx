import { useEffect, useLayoutEffect, useRef } from "react";
import { basicSetup } from "codemirror";
import { javascript } from "@codemirror/lang-javascript";
import { Compartment, EditorState, type Extension, type StateEffect, type TransactionSpec } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { MergeView } from "@codemirror/merge";
import type { DiffFile } from "../lib/types";
import styles from "../styles/App.module.css";

interface CodeSurfaceProps {
  content: string;
  filePath?: string;
  fontSize?: number;
  focusLine?: number;
  focusToken?: number;
}

function useStableCompartment(): Compartment {
  const compartmentRef = useRef<Compartment | null>(null);
  if (!compartmentRef.current) compartmentRef.current = new Compartment();
  return compartmentRef.current;
}

function isTypeScriptFile(filePath?: string): boolean {
  return Boolean(filePath?.endsWith(".ts") || filePath?.endsWith(".tsx"));
}

function languageExtension(filePath?: string): Extension {
  return javascript({ typescript: isTypeScriptFile(filePath) });
}

function sourceLabel(filePath?: string): string {
  return filePath ? `Read-only source for ${filePath}` : "Read-only source";
}

function scrollToLine(view: EditorView | null, focusLine?: number): void {
  if (!view || focusLine == null || !Number.isFinite(focusLine) || focusLine < 1) return;
  const lineNumber = Math.min(Math.floor(focusLine), view.state.doc.lines);
  const line = view.state.doc.line(lineNumber);
  view.dispatch({ effects: EditorView.scrollIntoView(line.from, { y: "center" }) });
}

function contentAccessibility(label: string): Extension {
  return EditorView.contentAttributes.of({ "aria-label": label });
}

function sourceExtensions(
  languageCompartment: Compartment,
  accessibilityCompartment: Compartment,
  filePath?: string,
): Extension[] {
  const label = sourceLabel(filePath);
  return [
    basicSetup,
    languageCompartment.of(languageExtension(filePath)),
    accessibilityCompartment.of(contentAccessibility(label)),
    EditorView.editable.of(false),
    EditorState.readOnly.of(true),
    EditorView.theme({ ".cm-content": { fontSize: "var(--cx-code-font-size, 13px)" } }),
  ];
}

function updateSourceView(
  view: EditorView,
  content: string,
  filePath: string | undefined,
  previousPath: string | undefined,
  previousTypeScript: boolean,
  languageCompartment: Compartment,
  accessibilityCompartment: Compartment,
): void {
  const current = view.state.doc;
  const changes = current.toString() === content ? undefined : { from: 0, to: current.length, insert: content };
  const nextTypeScript = isTypeScriptFile(filePath);
  const effects: StateEffect<unknown>[] = [];
  if (nextTypeScript !== previousTypeScript) effects.push(languageCompartment.reconfigure(languageExtension(filePath)));
  if (filePath !== previousPath) effects.push(accessibilityCompartment.reconfigure(contentAccessibility(sourceLabel(filePath))));
  if (!changes && effects.length === 0) return;

  const transaction: TransactionSpec = {};
  if (changes) transaction.changes = changes;
  if (effects.length > 0) transaction.effects = effects;
  view.dispatch(transaction);
}

export function CodeSurface({ content, filePath, fontSize = 13, focusLine, focusToken = 0 }: CodeSurfaceProps) {
  const host = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const languageCompartment = useStableCompartment();
  const accessibilityCompartment = useStableCompartment();
  const previousPathRef = useRef(filePath);
  const previousTypeScriptRef = useRef(isTypeScriptFile(filePath));

  useLayoutEffect(() => {
    host.current?.style.setProperty("--cx-code-font-size", `${fontSize}px`);
  }, [fontSize]);

  useEffect(() => {
    const parent = host.current;
    if (!parent) return;
    parent.style.setProperty("--cx-code-font-size", `${fontSize}px`);
    const view = new EditorView({
      state: EditorState.create({
        doc: content,
        extensions: sourceExtensions(languageCompartment, accessibilityCompartment, filePath),
      }),
      parent,
    });
    viewRef.current = view;
    return () => {
      view.destroy();
      if (viewRef.current === view) viewRef.current = null;
    };
    // The editor is intentionally created once per mounted host. Content and path updates are dispatched below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    updateSourceView(
      view,
      content,
      filePath,
      previousPathRef.current,
      previousTypeScriptRef.current,
      languageCompartment,
      accessibilityCompartment,
    );
    previousPathRef.current = filePath;
    previousTypeScriptRef.current = isTypeScriptFile(filePath);
  }, [content, filePath, languageCompartment, accessibilityCompartment]);

  useEffect(() => {
    scrollToLine(viewRef.current, focusLine);
  }, [focusLine, focusToken]);

  return (
    <div
      className={styles.codeSurface}
      ref={host}
      role="region"
      aria-label={sourceLabel(filePath)}
    />
  );
}

function diffContentAccessibility(label: string): Extension {
  return EditorView.contentAttributes.of({ "aria-label": label });
}

function diffExtensions(
  languageCompartment: Compartment,
  accessibilityCompartment: Compartment,
  label: string,
  filePath: string,
): Extension[] {
  return [
    basicSetup,
    languageCompartment.of(languageExtension(filePath)),
    accessibilityCompartment.of(diffContentAccessibility(label)),
    EditorView.editable.of(false),
    EditorState.readOnly.of(true),
    EditorView.theme({ ".cm-content": { fontSize: "var(--cx-code-font-size, 13px)" } }),
  ];
}

function updateDiffView(
  view: MergeView,
  file: DiffFile,
  previousPath: string,
  previousTypeScript: boolean,
  languageACompartment: Compartment,
  languageBCompartment: Compartment,
  accessibilityACompartment: Compartment,
  accessibilityBCompartment: Compartment,
): void {
  const pathChanged = file.path !== previousPath;
  const nextTypeScript = isTypeScriptFile(file.path);
  const languageChanged = nextTypeScript !== previousTypeScript;
  const aEffects: StateEffect<unknown>[] = [];
  const bEffects: StateEffect<unknown>[] = [];
  if (languageChanged) {
    const language = languageExtension(file.path);
    aEffects.push(languageACompartment.reconfigure(language));
    bEffects.push(languageBCompartment.reconfigure(language));
  }
  if (pathChanged) {
    aEffects.push(accessibilityACompartment.reconfigure(diffContentAccessibility(`Before source for ${file.path}`)));
    bEffects.push(accessibilityBCompartment.reconfigure(diffContentAccessibility(`After source for ${file.path}`)));
  }

  const updateEditor = (editor: EditorView, content: string, effects: StateEffect<unknown>[]) => {
    const transaction: TransactionSpec = {};
    if (editor.state.doc.toString() !== content) {
      transaction.changes = { from: 0, to: editor.state.doc.length, insert: content };
    }
    if (effects.length > 0) transaction.effects = effects;
    if (transaction.changes || transaction.effects) editor.dispatch(transaction);
  };
  updateEditor(view.a, file.before, aEffects);
  updateEditor(view.b, file.after, bEffects);
}

export function DiffSurface({ file, fontSize = 13 }: { file: DiffFile; fontSize?: number }) {
  const host = useRef<HTMLDivElement>(null);
  const mergeRef = useRef<MergeView | null>(null);
  const previousPathRef = useRef(file.path);
  const previousTypeScriptRef = useRef(isTypeScriptFile(file.path));
  const languageACompartment = useStableCompartment();
  const languageBCompartment = useStableCompartment();
  const accessibilityACompartment = useStableCompartment();
  const accessibilityBCompartment = useStableCompartment();

  useLayoutEffect(() => {
    host.current?.style.setProperty("--cx-code-font-size", `${fontSize}px`);
  }, [fontSize]);

  useEffect(() => {
    const parent = host.current;
    if (!parent) return;
    parent.style.setProperty("--cx-code-font-size", `${fontSize}px`);
    const view = new MergeView({
      a: {
        doc: file.before,
        extensions: diffExtensions(languageACompartment, accessibilityACompartment, `Before source for ${file.path}`, file.path),
      },
      b: {
        doc: file.after,
        extensions: diffExtensions(languageBCompartment, accessibilityBCompartment, `After source for ${file.path}`, file.path),
      },
      parent,
      highlightChanges: true,
      gutter: true,
    });
    mergeRef.current = view;
    return () => {
      view.destroy();
      if (mergeRef.current === view) mergeRef.current = null;
    };
    // The merge view is intentionally created once per mounted host. File values are dispatched below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const view = mergeRef.current;
    if (!view) return;
    updateDiffView(
      view,
      file,
      previousPathRef.current,
      previousTypeScriptRef.current,
      languageACompartment,
      languageBCompartment,
      accessibilityACompartment,
      accessibilityBCompartment,
    );
    previousPathRef.current = file.path;
    previousTypeScriptRef.current = isTypeScriptFile(file.path);
  }, [file, languageACompartment, languageBCompartment, accessibilityACompartment, accessibilityBCompartment]);

  return (
    <div
      className={styles.diffSurface}
      ref={host}
      role="region"
      aria-label={`Reviewed before and after diff for ${file.path}`}
    />
  );
}
