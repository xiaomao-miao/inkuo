import { useEffect, useState } from 'react';
import { Table, Button, Space, Tag, Modal, Form, Input, InputNumber, Switch, App } from 'antd';
import { PlusOutlined, EditOutlined, DeleteOutlined } from '@ant-design/icons';
import { plansApi, Plan } from '../api/plans';

export default function PlansPage() {
  const [data, setData] = useState<Plan[]>([]);
  const [loading, setLoading] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const [editing, setEditing] = useState<Plan | null>(null);
  const [form] = Form.useForm();
  const { message, modal } = App.useApp();

  const load = async () => {
    setLoading(true);
    try { setData(await plansApi.list()); } finally { setLoading(false); }
  };
  useEffect(() => { load(); }, []);

  const onSubmit = async (values: any) => {
    try {
      if (editing) {
        await plansApi.update(editing.id, values);
        message.success('套餐已更新');
      } else {
        await plansApi.create(values);
        message.success('套餐已创建');
      }
      setModalOpen(false);
      form.resetFields();
      setEditing(null);
      load();
    } catch (e: any) {
      message.error(e.response?.data?.error ?? '保存失败');
    }
  };

  const onDelete = (plan: Plan) => {
    modal.confirm({
      title: `删除套餐 "${plan.name}"？`,
      content: '如果该套餐还有订阅用户，将无法删除。',
      okType: 'danger',
      onOk: async () => {
        try {
          await plansApi.delete(plan.id);
          message.success('套餐已删除');
          load();
        } catch (e: any) {
          message.error(e.response?.data?.error ?? '删除失败');
        }
      },
    });
  };

  const columns = [
    { title: '名称', dataIndex: 'name', render: (n: string) => <Tag color="purple">{n}</Tag> },
    {
      title: '月费', dataIndex: 'monthlyPricePoints', width: 120,
      render: (p: number) => <strong>¥{(p / 1000).toFixed(3)}</strong>,
    },
    {
      title: '月 Token 额度', dataIndex: 'monthlyTokenLimit', width: 150,
      render: (t: number) => `${(t / 1000).toFixed(0)}K`,
    },
    {
      title: '超额单价 (元/1k)', width: 220,
      render: (_: any, r: Plan) => (
        <span>输入 {r.overageInputPricePer1k} · 输出 {r.overageOutputPricePer1k}</span>
      ),
    },
    {
      title: '订阅人数', dataIndex: 'subscriberCount', width: 110,
      render: (n: number) => <Tag color={n > 0 ? 'green' : 'default'}>{n}</Tag>,
    },
    {
      title: '状态', dataIndex: 'enabled', width: 90,
      render: (e: boolean) => e ? <Tag color="green">启用</Tag> : <Tag>停用</Tag>,
    },
    {
      title: '创建时间', dataIndex: 'createdAt', width: 160,
      render: (d: string) => new Date(d).toLocaleString(),
    },
    {
      title: '操作', width: 160, fixed: 'right' as const,
      render: (_: any, r: Plan) => (
        <Space>
          <Button size="small" icon={<EditOutlined />} onClick={() => {
            setEditing(r); form.setFieldsValue(r); setModalOpen(true);
          }}>编辑</Button>
          <Button size="small" danger icon={<DeleteOutlined />} onClick={() => onDelete(r)}>删除</Button>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <Space style={{ marginBottom: 16 }}>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => {
          setEditing(null); form.resetFields(); setModalOpen(true);
        }}>新增套餐</Button>
      </Space>

      <Table rowKey="id" loading={loading} dataSource={data} columns={columns} pagination={false} />

      <Modal
        title={editing ? `编辑套餐 - ${editing.name}` : '新增套餐'}
        open={modalOpen}
        onCancel={() => { setModalOpen(false); setEditing(null); }}
        onOk={() => form.submit()}
        width={600}
      >
        <Form form={form} layout="vertical" onFinish={onSubmit}
          initialValues={{ enabled: true, monthlyTokenLimit: 1_000_000, overageInputPricePer1k: 0.002, overageOutputPricePer1k: 0.004 }}>
          <Form.Item name="name" label="套餐名" rules={[{ required: true }]}>
            <Input placeholder="Free / Plus / Pro / Max" />
          </Form.Item>
          <Form.Item name="monthlyPricePoints" label="月费点数（1000 点 = ¥1，0 = 免费）" rules={[{ required: true }]}>
            <InputNumber min={0} style={{ width: '100%' }} step={1000} />
          </Form.Item>
          <Form.Item name="monthlyTokenLimit" label="月 Token 额度" rules={[{ required: true }]}>
            <InputNumber min={0} style={{ width: '100%' }} step={1_000_000} />
          </Form.Item>
          <Form.Item name="overageInputPricePer1k" label="超额输入单价 (元/1k)" rules={[{ required: true }]}>
            <InputNumber min={0} step={0.0005} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item name="overageOutputPricePer1k" label="超额输出单价 (元/1k)" rules={[{ required: true }]}>
            <InputNumber min={0} step={0.0005} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item name="enabled" label="启用" valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
