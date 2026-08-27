import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { ConfigProvider, App as AntdApp } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import 'dayjs/locale/zh-cn';
import App from './App';
import { ErrorBoundary } from './ErrorBoundary';
import './index.css';

function showUnhandledError(): void {
  if (document.getElementById('__global_error')) return;
  const el = document.createElement('div');
  el.id = '__global_error';
  el.className = 'global-error-toast';
  el.role = 'alert';
  el.textContent = '操作未完成，请稍后重试。';
  document.body.appendChild(el);
  window.setTimeout(() => el.remove(), 5000);
}

window.addEventListener('error', (event) => {
  console.error('[admin-window]', event.error ?? event.message);
  showUnhandledError();
});
window.addEventListener('unhandledrejection', (event) => {
  console.error('[admin-promise]', event.reason);
  showUnhandledError();
});

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ConfigProvider
      locale={zhCN}
      theme={{
        token: {
          colorPrimary: '#1677ff',
          borderRadius: 6,
        },
      }}
    >
      <AntdApp>
        <ErrorBoundary>
          <App />
        </ErrorBoundary>
      </AntdApp>
    </ConfigProvider>
  </StrictMode>
);
