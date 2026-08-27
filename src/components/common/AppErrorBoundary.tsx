import { Component, type ErrorInfo, type ReactNode } from 'react';
import { RefreshCw, TriangleAlert } from 'lucide-react';
import { reportError } from '../../utils/errors';
import styles from './AppErrorBoundary.module.css';

interface Props {
  children: ReactNode;
}

interface State {
  failed: boolean;
}

export class AppErrorBoundary extends Component<Props, State> {
  state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    reportError('app-render', { error, componentStack: info.componentStack });
  }

  render(): ReactNode {
    if (!this.state.failed) return this.props.children;

    return (
      <main className={styles.page} role="alert" aria-live="assertive">
        <section className={styles.card}>
          <span className={styles.icon} aria-hidden="true">
            <TriangleAlert size={24} />
          </span>
          <h1>界面遇到了一点问题</h1>
          <p>你的文件不会因此被删除。重新加载通常可以恢复工作区。</p>
          <button type="button" onClick={() => window.location.reload()}>
            <RefreshCw size={16} aria-hidden="true" />
            重新加载
          </button>
        </section>
      </main>
    );
  }
}
