import { memo, useEffect, useRef, useState } from "react";
import type { NodeProps, Node } from "@xyflow/react";
import { Button, Typography } from "antd";
import { useStore } from "../store";
import { backend } from "../lib/backend";

export type NoteNodeType = Node<{ text: string }, "note">;

/**
 * The shared notes as a card on the canvas: every agent reads and appends to
 * the same file (`hubcodes note show|add`); editing here writes it in place.
 */
export const NoteNode = memo(function NoteNode({ data, selected }: NodeProps<NoteNodeType>) {
  const notify = useStore((s) => s.notify);
  const toggleNotes = useStore((s) => s.toggleNotes);
  const [draft, setDraft] = useState(data.text);
  const [dirty, setDirty] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => {
    if (!dirty) setDraft(data.text);
  }, [data.text, dirty]);
  const save = (text: string) => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      backend.notesWrite(text).then(() => setDirty(false)).catch((e) => notify(`Could not save notes: ${e}`, "error"));
    }, 600);
  };
  return (
    <div className={`node note-node${selected ? " is-selected" : ""}`}>
      <header className="node-head">
        <span className="glyph">▤</span>
        <span className="node-name">notes</span>
        <Typography.Text type="secondary" className="note-hint">shared · agents: hubcodes note add "…"</Typography.Text>
        <Button type="text" size="small" className="nodrag" style={{ marginLeft: "auto" }} onClick={toggleNotes} title="Hide the notes card">Hide</Button>
      </header>
      <textarea
        className="note-text nodrag nowheel"
        value={draft}
        onChange={(e) => { setDraft(e.target.value); setDirty(true); save(e.target.value); }}
        onKeyDown={(e) => e.stopPropagation()}
        placeholder={"Decisions, paths, conventions — everything the team should remember.\n\nAgents read this with hubcodes note show and can append with hubcodes note add."}
        spellCheck={false}
        aria-label="Shared notes"
      />
      <footer className="node-foot"><Typography.Text type="secondary">{dirty ? "saving…" : "saved"}</Typography.Text><span className="node-links">{draft.split("\n").filter(Boolean).length} lines</span></footer>
    </div>
  );
});
