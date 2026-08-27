import {
  createBrowserRouter,
  RouterProvider,
  Navigate,
} from 'react-router-dom';
import { lazy, Suspense, useCallback, useEffect, useMemo, useState } from 'react';
import { Spin } from 'antd';
import { authApi, type AdminUser } from './api/auth';
import { tokenStore } from './api/client';
import LoginPage from './pages/Login';
import AdminLayout from './layouts/AdminLayout';

const DashboardPage = lazy(() => import('./pages/Dashboard'));
const UsersPage = lazy(() => import('./pages/Users'));
const PlansPage = lazy(() => import('./pages/Plans'));
const ModelConfigsPage = lazy(() => import('./pages/ModelConfigs'));
const WebSearchProvidersPage = lazy(() => import('./pages/WebSearchProviders'));
const InviteCodesPage = lazy(() => import('./pages/InviteCodes'));
const RedemptionCodesPage = lazy(() => import('./pages/RedemptionCodes'));
const UsagePage = lazy(() => import('./pages/Usage'));
const AdminsPage = lazy(() => import('./pages/Admins'));
const ReleasesPage = lazy(() => import('./pages/Releases'));

function FullPageSpinner() {
  return (
    <div className="full-page-spinner" role="status" aria-label="正在加载">
      <Spin size="large" tip="加载中..." />
    </div>
  );
}

export default function App() {
  const [admin, setAdmin] = useState<AdminUser | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const token = tokenStore.get();
    if (!token) {
      setLoading(false);
      return;
    }
    authApi.me()
      .then(setAdmin)
      .catch(() => tokenStore.clear())
      .finally(() => setLoading(false));
  }, []);

  const onLogout = useCallback(() => {
    tokenStore.clear();
    setAdmin(null);
  }, []);

  const router = useMemo(() => createBrowserRouter([
      {
        path: '/login',
        element: admin ? (
          <Navigate to="/" replace />
        ) : (
          <LoginPage onLogin={setAdmin} />
        ),
      },
      {
        element: admin ? (
          <Suspense fallback={<FullPageSpinner />}>
            <AdminLayout admin={admin} onLogout={onLogout} />
          </Suspense>
        ) : (
          <Navigate to="/login" replace />
        ),
        children: [
          { path: '/', element: <DashboardPage /> },
          { path: '/users', element: <UsersPage /> },
          { path: '/plans', element: <PlansPage /> },
          { path: '/models', element: <ModelConfigsPage /> },
          { path: '/web-search-providers', element: <WebSearchProvidersPage /> },
          { path: '/invite-codes', element: <InviteCodesPage /> },
          { path: '/redemption-codes', element: <RedemptionCodesPage /> },
          { path: '/usage', element: <UsagePage /> },
          { path: '/admins', element: <AdminsPage /> },
          { path: '/releases', element: <ReleasesPage /> },
        ],
      },
      { path: '*', element: <Navigate to="/" replace /> },
    ], { basename: '/admin' }), [admin, onLogout]);

  if (loading) {
    return <FullPageSpinner />;
  }

  return <RouterProvider router={router} />;
}
