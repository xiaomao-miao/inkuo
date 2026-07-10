import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { ConfigProvider, App as AntdApp } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import 'dayjs/locale/zh-cn';
import App from './App';
import './index.css';

// Catch unhandled errors so we don't just see minified stack traces
window.addEventListener('error', (e) => {
  if (!document.getElementById('__boot_error')) {
    const el = document.createElement('pre');
    el.id = '__boot_error';
    el.style.cssText = 'position:fixed;inset:0;background:#fff;color:#000;padding:24px;overflow:auto;z-index:99999;font-size:13px;white-space:pre-wrap;';
    el.textContent = `[BOOT ERROR]\n${e.message}\n\n${e.error?.stack ?? '(no stack)'}\n\nat ${e.filename}:${e.lineno}:${e.colno}`;
    document.body.appendChild(el);
  }
});
window.addEventListener('unhandledrejection', (e) => {
  const reason: any = e.reason;
  document.body.appendChild(Object.assign(document.createElement('pre'), {
    textContent: `[UNHANDLED REJECTION]\n${reason?.message ?? reason}\n\n${reason?.stack ?? ''}`,
    style: 'position:fixed;bottom:0;left:0;right:0;max-height:50%;background:#fee;color:#000;padding:16px;overflow:auto;z-index:99999;font-size:12px;white-space:pre-wrap;'
  }));
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
        <App />
      </AntdApp>
    </ConfigProvider>
  </StrictMode>
);