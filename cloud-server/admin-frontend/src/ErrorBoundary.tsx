import { Component, type ErrorInfo, type ReactNode } from 'react';
import { Button, Result } from 'antd';

interface Props {
  children: ReactNode;
}

interface State {
  failed: boolean;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error('[admin-render]', error, info.componentStack);
  }

  render(): ReactNode {
    if (!this.state.failed) return this.props.children;

    return (
      <Result
        status="500"
        title="页面暂时无法显示"
        subTitle="当前操作没有完成，请重新加载后再试。"
        extra={<Button type="primary" onClick={() => window.location.reload()}>重新加载</Button>}
      />
    );
  }
}
