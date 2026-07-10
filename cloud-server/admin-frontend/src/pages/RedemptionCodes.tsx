import { useEffect, useState } from 'react';
import {
  Table, Button, Space, Tag, Modal, Form, Input, InputNumber, Switch, DatePicker, Select, App, Popconfirm,
} from 'antd';
import { PlusOutlined, EditOutlined, DeleteOutlined, CopyOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { redemptionCodesApi, RedemptionCode } from '../api/redemptionCodes';
import { plansApi, Plan } from '../api/plans';

export default function RedemptionCodesPage() {
  const [data, setData] = useState<RedemptionCode[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(20);
  const [modalOpen, setModalOpen] = useState(false);
  const [editing, setEditing] = useState<RedemptionCode | null>(null);
  const [plans, setPlans] = useState<Plan[]>([]);
  const [form] = Form.useForm();
  const { message } = App.useApp();

  const load = async () => {
    setLoading(true);
    try {
      const [r, p] = await Promise.all([
        redemptionCodesApi.list(page, pageSize),
        plansApi.list(),
      ]);
      setData(r.items); setTotal(r.total); setPlans(p);
    } finally { setLoading(false); }
  };
  useEffect(() => { load(); }, [page, pageSize]);

  const onSubmit = async (values: any) => {
    try {
      const payload = {
        code: values.code,
        creditCents: values.creditCents ?? 0,
        planId: values.planId || null,
        maxUses: values.maxUses,
        expiresAt: values.expiresAt ? values.expiresAt.toISOString() : null,
        enabled: values.enabled,
      };
      if (editing) await redemptionCodesApi.update(editing.id, payload);
      else await redemptionCodesApi.create(payload);
      message.success(editing ? '兑换码已更新' : '兑换码已创建');
      setModalOpen(false); form.resetFields(); setEditing(null); load();
    } catch (e: any) {
      message.error(e.response?.data?.error ?? '保存失败');
    }
  };

  const onToggle = async (id: number) => {
    try {
      await redemptionCodesApi.toggle(id); message.success('状态已切换'); load();
    } catch (e: any) { message.error(e.response?.data?.error ?? '操作失败'); }
  };

  const onDelete = async (id: number) => {
    try {
      await redemptionCodesApi.delete(id); message.success('已删除'); load();
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
      title: '类型',
      width: 120,
      render: (_: any, r: RedemptionCode) => r.planId
        ? <Tag color="purple">套餐开通: {r.planName}</Tag>
        : <Tag color="blue">充值 ¥{(r.creditCents / 100).toFixed(2)}</Tag>,
    },
    {
      title: '使用情况', width: 160,
      render: (_: any, r: RedemptionCode) => (
        <Tag color={r.usedCount >= r.maxUses ? 'red' : r.usedCount > 0 ? 'blue' : 'default'}>
          {r.usedCount} / {r.maxUses}
        </Tag>
      ),
    },
    {
      title: '状态', dataIndex: 'enabled', width: 110,
      render: (e: boolean, r: RedemptionCode) => (
        <Popconfirm title={e ? '禁用该兑换码？' : '启用该兑换码？'} onConfirm={() => onToggle(r.id)}>
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
      render: (_: any, r: RedemptionCode) => (
        <Space>
          <Button size="small" icon={<EditOutlined />} onClick={() => {
            setEditing(r); form.setFieldsValue({
              ...r, expiresAt: r.expiresAt ? dayjs(r.expiresAt) : null, planId: r.planId ?? undefined,
            }); setModalOpen(true);
          }}>编辑</Button>
          <Button size="small" danger icon={<DeleteOutlined />} onClick={() => onDelete(r.id)}>删除</Button>
        </Space>
      ),
    },
  ];

  const planOptions = plans.map(p => ({ value: p.id, label: p.name }));

  return (
    <div>
      <Space style={{ marginBottom: 16 }}>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => {
          setEditing(null); form.resetFields(); setModalOpen(true);
        }}>新增兑换码</Button>
      </Space>

      <Table rowKey="id" loading={loading} dataSource={data} columns={columns}
        pagination={{ current: page, pageSize, total, onChange: setPage, showSizeChanger: true }} />

      <Modal
        title={editing ? `编辑兑换码 - ${editing.code}` : '新增兑换码'}
        open={modalOpen}
        onCancel={() => { setModalOpen(false); setEditing(null); }}
        onOk={() => form.submit()}
        width={600}
      >
        <Form form={form} layout="vertical" onFinish={onSubmit}
          initialValues={{ maxUses: 1, enabled: true, creditCents: 0 }}>
          <Form.Item name="code" label="兑换码 (建议大写)" rules={[{ required: true, min: 4 }]}>
            <Input placeholder="PLUS-MAR2026" style={{ textTransform: 'uppercase' }} />
          </Form.Item>
          <Form.Item name="planId" label="绑定套餐 (留空 = 纯充值)">
            <Select allowClear options={planOptions} placeholder="选择套餐则开通一个月, 否则只充值余额" />
          </Form.Item>
          <Form.Item name="creditCents" label="充值额度 (分, 套餐模式下可不填)">
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