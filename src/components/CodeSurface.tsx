import { useEffect, useRef } from "react";
import { basicSetup } from "codemirror";
import { javascript } from "@codemirror/lang-javascript";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { MergeView } from "@codemirror/merge";
import type { DiffFile } from "../lib/types";
import styles from "../styles/App.module.css";

interface CodeSurfaceProps { content: string; filePath?: string; fontSize?: number; focusLine?: number; focusToken?: number }

export function CodeSurface({ content, filePath, fontSize = 13, focusLine, focusToken = 0 }: CodeSurfaceProps) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!host.current) return;
    const view = new EditorView({
      state: EditorState.create({
        doc: content,
        extensions: [basicSetup, javascript({ typescript: Boolean(filePath?.endsWith(".ts") || filePath?.endsWith(".tsx")) }), EditorView.editable.of(false), EditorState.readOnly.of(true), EditorView.theme({ ".cm-content": { fontSize: `${fontSize}px` } })],
      }),
      parent: host.current,
    });
    if (focusLine) {
      const line = view.state.doc.line(Math.max(1, Math.min(focusLine, view.state.doc.lines)));
      view.dispatch({ effects: EditorView.scrollIntoView(line.from, { y: "center" }) });
    }
    return () => view.destroy();
  }, [content, filePath, fontSize, focusLine, focusToken]);
  return <div className={styles.codeSurface} ref={host} aria-label={filePath ? `Read-only source for ${filePath}` : "Read-only source"} />;
}

export function DiffSurface({ file, fontSize = 13 }: { file: DiffFile; fontSize?: number }) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!host.current) return;
    const extensions = [basicSetup, javascript({ typescript: Boolean(file.path.endsWith(".ts") || file.path.endsWith(".tsx")) }), EditorView.editable.of(false), EditorState.readOnly.of(true), EditorView.theme({ ".cm-content": { fontSize: `${fontSize}px` } })];
    const view = new MergeView({
      a: { doc: file.before, extensions },
      b: { doc: file.after, extensions },
      parent: host.current,
      highlightChanges: true,
      gutter: true,
    });
    return () => view.destroy();
  }, [file, fontSize]);
  return <div className={styles.diffSurface} ref={host} aria-label={`Reviewed before and after diff for ${file.path}`} />;
}
