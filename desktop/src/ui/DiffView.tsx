import { useMemo, useState } from "react";
import { Button, Input, Typography } from "antd";
import { DeleteOutlined, MessageOutlined, SendOutlined } from "@ant-design/icons";

const { Text } = Typography;

export interface DiffLine {
  /** Index in the raw diff. */
  i: number;
  text: string;
  kind: "add" | "del" | "ctx" | "hunk" | "file" | "meta";
  file: string;
  /** Line number in the new file (adds and context), or the old one for deletions. */
  line: number | null;
}

/** Walks a unified diff and assigns each line a file and a line number. */
export function parseDiff(diff: string): DiffLine[] {
  const out: DiffLine[] = [];
  let file = "";
  let newLine = 0;
  let oldLine = 0;
  diff.split("\n").forEach((text, i) => {
    if (text.startsWith("diff ")) {
      const m = /b\/(.+)$/.exec(text);
      file = m ? m[1] : text.slice(5);
      out.push({ i, text, kind: "file", file, line: null });
      return;
    }
    if (text.startsWith("@@")) {
      const m = /@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(text);
      oldLine = m ? Number(m[1]) : 0;
      newLine = m ? Number(m[2]) : 0;
      out.push({ i, text, kind: "hunk", file, line: null });
      return;
    }
    if (text.startsWith("+++") || text.startsWith("---") || text.startsWith("index ") || text.startsWith("new file") || text.startsWith("deleted file") || text.startsWith("similarity") || text.startsWith("rename ")) {
      out.push({ i, text, kind: "meta", file, line: null });
      return;
    }
    if (text.startsWith("+")) { out.push({ i, text, kind: "add", file, line: newLine }); newLine += 1; return; }
    if (text.startsWith("-")) { out.push({ i, text, kind: "del", file, line: oldLine }); oldLine += 1; return; }
    out.push({ i, text, kind: "ctx", file, line: newLine });
    if (text !== "" || i < diff.length) { newLine += 1; oldLine += 1; }
  });
  return out;
}

export interface Comment {
  key: number;
  file: string;
  line: number | null;
  code: string;
  text: string;
}

/** The review as one message the agent can act on. */
export function reviewMessage(branch: string, comments: Comment[]): string {
  const head = `Review of your changes on ${branch}: ${comments.length} comment${comments.length === 1 ? "" : "s"}. Address each, then reply with what you changed.`;
  const body = comments.map((c, n) => `${n + 1}. ${c.file}${c.line ? `:${c.line}` : ""}\n   > ${c.code.trim()}\n   ${c.text.trim()}`).join("\n\n");
  return `${head}\n\n${body}`;
}

/**
 * The diff with line numbers; click a line to leave a comment, then send
 * them all to the agent as one message. Comments stay until sent or cleared.
 */
export function DiffView({ diff, branch, onSend }: { diff: string; branch: string; onSend: (text: string) => void }) {
  const lines = useMemo(() => parseDiff(diff), [diff]);
  const [comments, setComments] = useState<Comment[]>([]);
  const [draftAt, setDraftAt] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const commit = (l: DiffLine) => {
    if (!draft.trim()) { setDraftAt(null); return; }
    setComments((cs) => [...cs.filter((c) => c.key !== l.i), { key: l.i, file: l.file, line: l.line, code: l.text.slice(1), text: draft }]);
    setDraft("");
    setDraftAt(null);
  };
  const byLine = new Map(comments.map((c) => [c.key, c]));
  if (!diff.trim()) return <pre className="diff">No changes.</pre>;
  return (
    <div className="diffview">
      {comments.length > 0 && (
        <div className="review-bar">
          <Text>{comments.length} review comment{comments.length === 1 ? "" : "s"}</Text>
          <span className="grow" />
          <Button size="small" onClick={() => setComments([])}>Clear</Button>
          <Button size="small" type="primary" icon={<SendOutlined />} onClick={() => { onSend(reviewMessage(branch, comments)); setComments([]); }}>Send to agent</Button>
        </div>
      )}
      <pre className="diff">
        {lines.map((l) => {
          const c = byLine.get(l.i);
          const commentable = l.kind === "add" || l.kind === "del" || l.kind === "ctx";
          return (
            <div key={l.i} className={`d-line d-${l.kind}${c ? " has-comment" : ""}${draftAt === l.i ? " is-drafting" : ""}`}>
              <span className="d-no">{l.line ?? ""}</span>
              <span className="d-text" onClick={() => { if (commentable) { setDraftAt(l.i); setDraft(c?.text ?? ""); } }} title={commentable ? "Click to comment" : undefined}>{l.text || " "}</span>
              {commentable && !c && draftAt !== l.i && <button type="button" className="d-add-comment" aria-label="Comment on this line" onClick={() => { setDraftAt(l.i); setDraft(""); }}><MessageOutlined /></button>}
              {draftAt === l.i && (
                <div className="d-comment-box">
                  <Input.TextArea autoFocus autoSize={{ minRows: 1, maxRows: 5 }} value={draft} placeholder={`Comment on ${l.file}${l.line ? `:${l.line}` : ""}`} onChange={(e) => setDraft(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); commit(l); } if (e.key === "Escape") { e.preventDefault(); setDraftAt(null); } e.stopPropagation(); }} />
                  <div className="d-comment-actions">
                    <Button size="small" type="primary" onClick={() => commit(l)} disabled={!draft.trim()}>Add</Button>
                    <Button size="small" onClick={() => setDraftAt(null)}>Cancel</Button>
                  </div>
                </div>
              )}
              {c && draftAt !== l.i && (
                <div className="d-comment">
                  <span className="d-comment-text">{c.text}</span>
                  <Button size="small" type="text" danger icon={<DeleteOutlined />} aria-label="Remove comment" onClick={() => setComments((cs) => cs.filter((x) => x.key !== l.i))} />
                </div>
              )}
            </div>
          );
        })}
      </pre>
    </div>
  );
}
