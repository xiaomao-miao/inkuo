import { useState } from 'react';
import { Form, Input, Button, Card, Typography, App, Alert } from 'antd';
import { UserOutlined, LockOutlined } from '@ant-design/icons';
import { authApi } from '../api/auth';
import { tokenStore } from '../api/client';
import { AdminUser } from '../api/auth';

const { Title, Text } = Typography;

interface Props {
  onLogin: (admin: AdminUser) => void;
}

export default function LoginPage({ onLogin }: Props) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const { message } = App.useApp();

  const onFinish = async (values: { username: string; password: string }) => {
    setLoading(true);
    setError('');
    try {
      const result = await authApi.login(values.username, values.password);
      tokenStore.set(result.accessToken);
      message.success('登录成功');
      onLogin(result.admin);
    } catch (err: any) {
      if (err.response?.status === 401) {
        setError('用户名或密码错误');
      } else {
        setError(err.response?.data?.error ?? '登录失败，请稍后重试');
      }
    } finally {
      setLoading(false);
    }
  };

  return (
    <div
      style={{
        minHeight: '100vh',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        background: 'linear-gradient(135deg, #667eea 0%, #764ba2 100%)',
      }}
    >
      <Card style={{ width: 400, boxShadow: '0 10px 30px rgba(0,0,0,0.15)' }}>
        <div style={{ textAlign: 'center', marginBottom: 24 }}>
          <Title level={2} style={{ marginBottom: 4 }}>inkuo Cloud</Title>
          <Text type="secondary">管理面板</Text>
        </div>

        {error && <Alert type="error" message={error} style={{ marginBottom: 16 }} />}

        <Form layout="vertical" onFinish={onFinish} autoComplete="off">
          <Form.Item name="username" rules={[{ required: true, message: '请输入用户名' }]}>
            <Input prefix={<UserOutlined />} placeholder="用户名" size="large" />
          </Form.Item>
          <Form.Item name="password" rules={[{ required: true, message: '请输入密码' }]}>
            <Input.Password prefix={<LockOutlined />} placeholder="密码" size="large" />
          </Form.Item>
          <Form.Item>
            <Button type="primary" htmlType="submit" loading={loading} size="large" block>
              登录
            </Button>
          </Form.Item>
        </Form>

        <Text type="secondary" style={{ display: 'block', textAlign: 'center', fontSize: 12 }}>
          默认账号: admin / admin123 (首次登录后请立即修改)
        </Text>
      </Card>
    </div>
  );
}