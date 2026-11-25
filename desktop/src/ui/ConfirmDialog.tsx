import { useEffect, useRef } from "react";
import { Button, Space, Typography } from "antd";
import { useStore } from "../store";
import { Dialog } from "./Dialog";

/** One question, one destructive button. Enter confirms, Esc cancels. */
export function ConfirmDialog() {
  const confirm = useStore((s) => s.confirm);
  const setConfirm = useStore((s) => s.setConfirm);
  const btn = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (confirm) btn.current?.focus();
  }, [confirm]);
  if (!confirm) return null;
  const close = () => setConfirm(null);
  return (
    <Dialog title={confirm.title} onClose={close} width={440}>
      <Typography.Paragraph className="confirm-body">{confirm.body}</Typography.Paragraph>
      <div className="dialog-actions">
        <Space>
          <Button onClick={close}>Cancel</Button>
          <Button ref={btn} type="primary" danger onClick={() => { confirm.run(); close(); }}>{confirm.label}</Button>
        </Space>
      </div>
    </Dialog>
  );
}
