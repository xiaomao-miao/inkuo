import { useCallback, useEffect, useRef, useState } from 'react';
import { Table, Button, Space, Tag, Modal, Form, Input, Select, App, type TableColumnsType } from 'antd';
import { PlusOutlined, DeleteOutlined, CrownOutlined, UserOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { authApi } from '../api/auth';
import { getApiErrorMessage } from '../api/client';
import { validateNewPassword } from '../passwordPolicy';

interface AdminRow {
  id: string;
  username: string;
  role: string;
  enabled: boolean;
  createdAt: string;
  lastLoginAt: string | null;
}

export default function AdminsPage() {
  const [data, setData] = useState<AdminRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [form] = Form.useForm();
  const { message, modal } = App.useApp();
  const requestIdRef = useRef(0);

  const load = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setLoading(true);
    try {
      const result = await authApi.listAdmins();
      if (requestId === requestIdRef.current) setData(result);
    } catch (error) {
      if (requestId === requestIdRef.current) {
        message.error(getApiErrorMessage(error, '加载管理员失败'));
      }
    } finally {
      if (requestId === requestIdRef.current) setLoading(false);
    }
  }, [message]);
  useEffect(() => {
    void load();
    return () => { requestIdRef.current += 1; };
  }, [load]);

  const onCreate = async (values: any) => {
    setSaving(true);
    try {
      await authApi.createAdmin(values.username, values.password, values.role);
      message.success('管理员已创建');
      setModalOpen(false); form.resetFields(); await load();
    } catch (error) {
      message.error(getApiErrorMessage(error, '创建失败'));
    } finally {
      setSaving(false);
    }
  };

  const onDelete = (admin: AdminRow) => {
    modal.confirm({
      title: `删除管理员 "${admin.username}"？`,
      content: '不可恢复。',
      okType: 'danger',
      onOk: async () => {
        try {
          await authApi.deleteAdmin(admin.id);
          message.success('已删除'); await load();
        } catch (error) {
          message.error(getApiErrorMessage(error, '删除失败'));
        }
      },
    });
  };

  const columns: TableColumnsType<AdminRow> = [
    {
      title: '用户名', dataIndex: 'username',
      render: (u: string, r: AdminRow) => (
        <Space>
          {r.role === 'superadmin' ? <CrownOutlined style={{ color: '#faad14' }} /> : <UserOutlined />}
          <strong>{u}</strong>
        </Space>
      ),
    },
    {
      title: '角色', dataIndex: 'role', width: 140,
      render: (r: string) => <Tag color={r === 'superadmin' ? 'gold' : 'blue'}>{r}</Tag>,
    },
    {
      title: '状态', dataIndex: 'enabled', width: 100,
      render: (e: boolean) => <Tag color={e ? 'green' : 'red'}>{e ? '启用' : '停用'}</Tag>,
    },
    {
      title: '创建时间', dataIndex: 'createdAt', width: 160,
      render: (d: string) => dayjs(d).format('YYYY-MM-DD HH:mm'),
    },
    {
      title: '最后登录', dataIndex: 'lastLoginAt', width: 160,
      render: (d: string | null) => d ? dayjs(d).format('YYYY-MM-DD HH:mm') : <Tag>从未</Tag>,
    },
    {
      title: '操作', width: 120,
      render: (_: any, r: AdminRow) => (
        <Button danger size="small" icon={<DeleteOutlined />} onClick={() => onDelete(r)}>
          删除
        </Button>
      ),
    },
  ];

  return (
    <div>
      <Space style={{ marginBottom: 16 }}>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => { form.resetFields(); setModalOpen(true); }}>
          新增管理员
        </Button>
      </Space>

      <Table rowKey="id" loading={loading} dataSource={data} columns={columns} pagination={false} />

      <Modal title="新增管理员" open={modalOpen} onCancel={() => setModalOpen(false)} onOk={() => form.submit()} confirmLoading={saving}>
        <Form form={form} layout="vertical" onFinish={onCreate} initialValues={{ role: 'admin' }}>
          <Form.Item name="username" label="用户名" rules={[{ required: true, min: 3 }]}>
            <Input />
          </Form.Item>
          <Form.Item name="password" label="密码" rules={[
            { required: true },
            {
              validator: (_, value?: string) => {
                if (!value) return Promise.resolve();
                return validateNewPassword(value);
              },
            },
          ]}>
            <Input.Password />
          </Form.Item>
          <Form.Item name="role" label="角色">
            <Select options={[
              { value: 'admin', label: 'admin - 普通管理员' },
              { value: 'superadmin', label: 'superadmin - 超级管理员 (可管理其他管理员)' },
            ]} />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
