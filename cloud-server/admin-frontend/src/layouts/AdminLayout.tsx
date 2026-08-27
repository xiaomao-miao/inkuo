import { Outlet, useNavigate, useLocation } from 'react-router-dom';
import { Layout, Menu, Dropdown, Avatar, Space, Modal, Form, Input, App } from 'antd';
import {
  DashboardOutlined,
  UserOutlined,
  CrownOutlined,
  ApiOutlined,
  GiftOutlined,
  TagsOutlined,
  BarChartOutlined,
  TeamOutlined,
  LogoutOutlined,
  KeyOutlined,
  GlobalOutlined,
  CloudUploadOutlined,
} from '@ant-design/icons';
import { useState } from 'react';
import { type AdminUser, authApi } from '../api/auth';
import { getApiErrorMessage } from '../api/client';
import { validateNewPassword } from '../passwordPolicy';

const { Header, Sider, Content } = Layout;

interface Props {
  admin: AdminUser;
  onLogout: () => void;
}

export default function AdminLayout({ admin, onLogout }: Props) {
  const navigate = useNavigate();
  const location = useLocation();
  const { message } = App.useApp();
  const [pwModalOpen, setPwModalOpen] = useState(false);
  const [changingPassword, setChangingPassword] = useState(false);
  const [pwForm] = Form.useForm();

  const menuItems = [
    { key: '/', icon: <DashboardOutlined />, label: '仪表盘' },
    { key: '/users', icon: <UserOutlined />, label: '用户管理' },
    { key: '/plans', icon: <CrownOutlined />, label: '套餐管理' },
    { key: '/models', icon: <ApiOutlined />, label: '模型配置' },
    { key: '/web-search-providers', icon: <GlobalOutlined />, label: '联网搜索' },
    { key: '/invite-codes', icon: <GiftOutlined />, label: '邀请码' },
    { key: '/redemption-codes', icon: <TagsOutlined />, label: '兑换码' },
    { key: '/usage', icon: <BarChartOutlined />, label: '用量记录' },
    { key: '/releases', icon: <CloudUploadOutlined />, label: '发行版' },
    { key: '/admins', icon: <TeamOutlined />, label: '管理员' },
  ];

  const userMenu = {
    items: [
      { key: 'change-pw', icon: <KeyOutlined />, label: '修改密码' },
      { type: 'divider' as const },
      { key: 'logout', icon: <LogoutOutlined />, label: '退出登录', danger: true },
    ],
    onClick: async ({ key }: { key: string }) => {
      if (key === 'change-pw') setPwModalOpen(true);
      else if (key === 'logout') onLogout();
    },
  };

  const onChangePassword = async (values: { currentPassword: string; newPassword: string }) => {
    setChangingPassword(true);
    try {
      await authApi.changePassword(values.currentPassword, values.newPassword);
      message.success('密码已更新');
      setPwModalOpen(false);
      pwForm.resetFields();
    } catch (error) {
      message.error(getApiErrorMessage(error, '修改失败'));
    } finally {
      setChangingPassword(false);
    }
  };

  return (
    <Layout style={{ minHeight: '100vh' }}>
      <Sider theme="dark" width={220}>
        <div style={{ color: '#fff', textAlign: 'center', padding: '20px 0', fontSize: 18, fontWeight: 600 }}>
          inkuo Cloud
          <div style={{ fontSize: 11, fontWeight: 400, opacity: 0.6, marginTop: 4 }}>
            <a href="/" target="_blank" rel="noreferrer" style={{ color: 'inherit' }}>查看首页 ↗</a>
          </div>
        </div>
        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={[location.pathname]}
          items={menuItems}
          onClick={({ key }) => navigate(key)}
        />
      </Sider>
      <Layout>
        <Header
          style={{
            background: '#fff',
            padding: '0 24px',
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            boxShadow: '0 1px 4px rgba(0,21,41,0.08)',
          }}
        >
          <div style={{ fontSize: 16, fontWeight: 500 }}>
            {menuItems.find(m => m.key === location.pathname)?.label ?? 'inkuo Cloud'}
          </div>
          <Dropdown menu={userMenu} placement="bottomRight">
            <Space style={{ cursor: 'pointer' }}>
              <Avatar style={{ backgroundColor: '#1677ff' }}>{admin.username[0]?.toUpperCase()}</Avatar>
              <span>{admin.username}</span>
              {admin.role === 'superadmin' && <CrownOutlined style={{ color: '#faad14' }} />}
            </Space>
          </Dropdown>
        </Header>
        <Content style={{ margin: 24, padding: 24, background: '#fff', borderRadius: 8 }}>
          <Outlet />
        </Content>
      </Layout>

      <Modal
        title="修改密码"
        open={pwModalOpen}
        onCancel={() => setPwModalOpen(false)}
        onOk={() => pwForm.submit()}
        confirmLoading={changingPassword}
      >
        <Form form={pwForm} layout="vertical" onFinish={onChangePassword}>
          <Form.Item name="currentPassword" label="当前密码" rules={[{ required: true }]}>
            <Input.Password />
          </Form.Item>
          <Form.Item name="newPassword" label="新密码" rules={[
            { required: true },
            { validator: (_, value?: string) => value ? validateNewPassword(value) : Promise.resolve() },
          ]}>
            <Input.Password />
          </Form.Item>
        </Form>
      </Modal>
    </Layout>
  );
}
