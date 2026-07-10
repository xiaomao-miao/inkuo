import {
  createBrowserRouter,
  RouterProvider,
  Navigate,
} from 'react-router-dom';
import { useEffect, useState } from 'react';
import { Spin } from 'antd';
import { authApi, AdminUser } from './api/auth';
import { tokenStore } from './api/client';
import LoginPage from './pages/Login';
import AdminLayout from './layouts/AdminLayout';
import DashboardPage from './pages/Dashboard';
import UsersPage from './pages/Users';
import PlansPage from './pages/Plans';
import ModelConfigsPage from './pages/ModelConfigs';
import InviteCodesPage from './pages/InviteCodes';
import RedemptionCodesPage from './pages/RedemptionCodes';
import UsagePage from './pages/Usage';
import AdminsPage from './pages/Admins';

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

  if (loading) {
    return (
      <div style={{ display: 'flex', justifyContent: 'center', alignItems: 'center', height: '100vh' }}>
        <Spin size="large" tip="加载中..." />
      </div>
    );
  }

  const onLogout = () => {
    tokenStore.clear();
    setAdmin(null);
  };

  const router = createBrowserRouter([
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
        <AdminLayout admin={admin} onLogout={onLogout} />
      ) : (
        <Navigate to="/login" replace />
      ),
      children: [
        { path: '/', element: <DashboardPage /> },
        { path: '/users', element: <UsersPage /> },
        { path: '/plans', element: <PlansPage /> },
        { path: '/models', element: <ModelConfigsPage /> },
        { path: '/invite-codes', element: <InviteCodesPage /> },
        { path: '/redemption-codes', element: <RedemptionCodesPage /> },
        { path: '/usage', element: <UsagePage /> },
        { path: '/admins', element: <AdminsPage /> },
      ],
    },
    { path: '*', element: <Navigate to="/" replace /> },
  ]);

  return <RouterProvider router={router} />;
}