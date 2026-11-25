import { useEffect, type ReactNode } from "react";
import { Button, Typography } from "antd";

/**
 * Dialog shell: scrim, card, title, Esc. Ant Design popups inside it
 * (Select, Dropdown, Drawer, Popconfirm) take the first Escape themselves.
 */
export function Dialog({ title, onClose, children, width = 560, className, extra }: { title: ReactNode; onClose: () => void; children: ReactNode; width?: number; className?: string; extra?: ReactNode }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (document.querySelector(".ant-select-open, .ant-dropdown:not(.ant-dropdown-hidden), .ant-drawer-open, .ant-popover:not(.ant-popover-hidden), .ant-modal-wrap:not([style*=\"display: none\"]), .ant-select-dropdown:not(.ant-select-dropdown-hidden)")) return;
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);
  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={`dialog${className ? ` ${className}` : ""}`} role="dialog" aria-modal="true" aria-label={typeof title === "string" ? title : undefined} style={{ width }}>
        <header className="dialog-head">
          <Typography.Title level={5} className="dialog-title">{title}</Typography.Title>
          {extra}
          <Button type="text" size="small" onClick={onClose} aria-label="Close">Esc</Button>
        </header>
        <div className="dialog-body">{children}</div>
      </div>
    </div>
  );
}
