import { Component, type ErrorInfo, type ReactNode } from "react";

interface ApplicationErrorBoundaryProps {
  children: ReactNode;
  onReload?: () => void;
}

interface ApplicationErrorBoundaryState {
  failed: boolean;
}

export class ApplicationErrorBoundary extends Component<
  ApplicationErrorBoundaryProps,
  ApplicationErrorBoundaryState
> {
  state: ApplicationErrorBoundaryState = { failed: false };

  static getDerivedStateFromError(): ApplicationErrorBoundaryState {
    return { failed: true };
  }

  componentDidCatch(_error: Error, _errorInfo: ErrorInfo) {
    console.error("HAL100 renderer failed: renderer_runtime_error");
  }

  render() {
    if (!this.state.failed) {
      return this.props.children;
    }

    return (
      <main className="startup-failure" role="alert">
        <section className="startup-failure-card">
          <span aria-hidden="true" className="startup-failure-badge">
            !
          </span>
          <div>
            <h1>HAL100 界面发生异常</h1>
            <p>本地核心与数据不会被重置。请重新加载界面；如果问题持续出现，可检查应用日志。</p>
            <code>诊断代码：renderer_runtime_error</code>
            <button onClick={this.props.onReload ?? (() => window.location.reload())} type="button">
              重新加载界面
            </button>
          </div>
        </section>
      </main>
    );
  }
}
