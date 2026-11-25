import { Component, type ErrorInfo, type ReactNode } from "react";

/** A render error in one part of the UI shows a message instead of a blank window. */
export class ErrorBoundary extends Component<{ children: ReactNode; label?: string; onReset?: () => void }, { error: Error | null }> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("[hubcodes] ui error", error, info.componentStack);
  }
  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="ui-error" role="alert">
        <h2>{this.props.label ?? "This part of hubcodes"} hit an error</h2>
        <pre>{String(error?.message ?? error)}</pre>
        <div className="row" style={{ gap: 8 }}>
          {this.props.onReset && <button className="btn ghost" onClick={() => { this.setState({ error: null }); this.props.onReset?.(); }}>Close</button>}
          <button className="btn primary" onClick={() => window.location.reload()}>Reload hubcodes</button>
        </div>
      </div>
    );
  }
}
