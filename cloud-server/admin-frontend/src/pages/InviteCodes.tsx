import { useEffect, useState } from 'react';
import {
  Table, Button, Space, Tag, Modal, Form, Input, InputNumber, Switch, DatePicker, App, Popconfirm,
} from 'antd';
import { PlusOutlined, EditOutlined, DeleteOutlined, CopyOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { inviteCodesApi, InviteCode } from '../api/inviteCodes';

export default function InviteCodesPage() {
  const [data, setData] = useState<InviteCode[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(20);
  const [modalOpen, setModalOpen] = useState(false);
  const [editing, setEditing] = useState<InviteCode | null>(null);
  const [form] = Form.useForm();
  const { message } = App.useApp();

  const load = async () => {
    setLoading(true);
    try {
      const r = await inviteCodesApi.list(page, pageSize);
      setData(r.items); setTotal(r.total);
    } finally { setLoading(false); }
  };
  useEffect(() => { load(); }, [page, pageSize]);

  const onSubmit = async (values: any) => {
    try {
      const payload = {
        ...values,
        expiresAt: values.expiresAt ? values.expiresAt.toISOString() : null,
      };
      if (editing) await inviteCodesApi.update(editing.id, payload);
      else await inviteCodesApi.create(payload);
      message.success(editing ? '邀请码已更新' : '邀请码已创建');
      setModalOpen(false); form.resetFields(); setEditing(null); load();
    } catch (e: any) {
      message.error(e.response?.data?.error ?? '保存失败');
    }
  };

  const onToggle = async (id: number) => {
    try {
      await inviteCodesApi.toggle(id); message.success('状态已切换'); load();
    } catch (e: any) { message.error(e.response?.data?.error ?? '操作失败'); }
  };

  const onDelete = async (id: number) => {
    try {
      await inviteCodesApi.delete(id); message.success('已删除'); load();
    } catch (e: any) { message.error(e.response?.data?.error ?? '删除失败'); }
  };

  const copy = (text: string) => {
    navigator.clipboard.writeText(text);
    message.success(`已复制: ${text}`);
  };

  const columns = [
    {
      title: '代码', dataIndex: 'code', width: 220,
      render: (c: string) => (
        <Space>
          <code style={{ fontSize: 14, fontWeight: 600 }}>{c}</code>
          <Button size="small" type="text" icon={<CopyOutlined />} onClick={() => copy(c)} />
        </Space>
      ),
    },
    {
      title: '赠送额度 (元)', dataIndex: 'freeQuotaCents', width: 130,
      render: (c: number) => <strong>¥{(c / 100).toFixed(2)}</strong>,
    },
    {
      title: '使用情况', width: 180,
      render: (_: any, r: InviteCode) => (
        <Tag color={r.usedCount >= r.maxUses ? 'red' : r.usedCount > 0 ? 'blue' : 'default'}>
          {r.usedCount} / {r.maxUses}
        </Tag>
      ),
    },
    {
      title: '状态', dataIndex: 'enabled', width: 110,
      render: (e: boolean, r: InviteCode) => (
        <Popconfirm title={e ? '禁用该邀请码？' : '启用该邀请码？'} onConfirm={() => onToggle(r.id)}>
          <Tag color={e ? 'green' : 'default'} style={{ cursor: 'pointer' }}>
            {e ? '启用' : '停用'} (点击切换)
          </Tag>
        </Popconfirm>
      ),
    },
    {
      title: '过期时间', dataIndex: 'expiresAt', width: 150,
      render: (d: string | null) => d ? dayjs(d).format('YYYY-MM-DD') : <Tag>永久</Tag>,
    },
    {
      title: '创建时间', dataIndex: 'createdAt', width: 160,
      render: (d: string) => dayjs(d).format('YYYY-MM-DD HH:mm'),
    },
    {
      title: '操作', width: 160, fixed: 'right' as const,
      render: (_: any, r: InviteCode) => (
        <Space>
          <Button size="small" icon={<EditOutlined />} onClick={() => {
            setEditing(r); form.setFieldsValue({ ...r, expiresAt: r.expiresAt ? dayjs(r.expiresAt) : null }); setModalOpen(true);
          }}>编辑</Button>
          <Button size="small" danger icon={<DeleteOutlined />} onClick={() => onDelete(r.id)}>删除</Button>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <Space style={{ marginBottom: 16 }}>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => {
          setEditing(null); form.resetFields(); setModalOpen(true);
        }}>新增邀请码</Button>
      </Space>

      <Table rowKey="id" loading={loading} dataSource={data} columns={columns}
        pagination={{
          current: page,
          pageSize,
          total,
          onChange: (nextPage, nextPageSize) => {
            setPage(nextPage);
            setPageSize(nextPageSize);
          },
          showSizeChanger: true,
        }} />

      <Modal
        title={editing ? `编辑邀请码 - ${editing.code}` : '新增邀请码'}
        open={modalOpen}
        onCancel={() => { setModalOpen(false); setEditing(null); }}
        onOk={() => form.submit()}
      >
        <Form form={form} layout="vertical" onFinish={onSubmit}
          initialValues={{ maxUses: 1, freeQuotaCents: 100, enabled: true }}>
          <Form.Item name="code" label="邀请码 (建议大写)" rules={[{ required: true, min: 4 }]}>
            <Input placeholder="BETA2026" style={{ textTransform: 'uppercase' }} />
          </Form.Item>
          <Form.Item name="freeQuotaCents" label="注册免费赠送 (分)" rules={[{ required: true }]}>
            <InputNumber min={0} style={{ width: '100%' }} step={100} />
          </Form.Item>
          <Form.Item name="maxUses" label="最大使用次数" rules={[{ required: true }]}>
            <InputNumber min={1} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item name="expiresAt" label="过期时间 (留空 = 永久)">
            <DatePicker style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item name="enabled" label="启用" valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
