import { useEffect, useState } from 'react';
import {
  Table, Button, Space, Tag, Modal, Form, Input, InputNumber, Switch, Select, App, Tooltip,
} from 'antd';
import { PlusOutlined, EditOutlined, DeleteOutlined } from '@ant-design/icons';
import { modelConfigsApi, ModelConfig } from '../api/modelConfigs';

const providerOptions = [
  { value: 'openai', label: 'OpenAI 兼容协议（OpenAI / 月之暗面 / vLLM / Ollama 等）' },
  { value: 'deepseek', label: 'DeepSeek（OpenAI 兼容协议）' },
];

export default function ModelConfigsPage() {
  const [data, setData] = useState<ModelConfig[]>([]);
  const [loading, setLoading] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const [editing, setEditing] = useState<ModelConfig | null>(null);
  const [form] = Form.useForm();
  const { message, modal } = App.useApp();

  const load = async () => {
    setLoading(true);
    try {
      setData(await modelConfigsApi.list());
    } finally {
      setLoading(false);
    }
  };
  useEffect(() => { load(); }, []);

  const onSubmit = async (values: any) => {
    try {
      const payload = { ...values, sortOrder: Number(values.sortOrder ?? 0) };
      if (editing) {
        // For update, blank key means "keep existing"
        if (!payload.upstreamApiKey) delete payload.upstreamApiKey;
        await modelConfigsApi.update(editing.id, payload);
        message.success('模型已更新');
      } else {
        if (!payload.upstreamApiKey) {
          message.warning('请输入上游 API Key');
          return;
        }
        await modelConfigsApi.create(payload);
        message.success('模型已创建');
      }
      setModalOpen(false);
      form.resetFields();
      setEditing(null);
      load();
    } catch (e: any) {
      message.error(e.response?.data?.error ?? '保存失败');
    }
  };

  const onDelete = (m: ModelConfig) => {
    modal.confirm({
      title: `删除模型 "${m.displayName}"？`,
      content: '如果该模型已有用量记录，将无法删除。',
      okType: 'danger',
      onOk: async () => {
        try {
          await modelConfigsApi.delete(m.id);
          message.success('已删除');
          load();
        } catch (e: any) {
          message.error(e.response?.data?.error ?? '删除失败');
        }
      },
    });
  };

  const columns = [
    { title: '排序', dataIndex: 'sortOrder', width: 60 },
    {
      title: '显示名', dataIndex: 'displayName', width: 160,
      render: (n: string, r: ModelConfig) => (
        <Space>
          <strong>{n}</strong>
          <Tag color={r.enabled ? 'green' : 'default'}>{r.enabled ? '启用' : '停用'}</Tag>
        </Space>
      ),
    },
    { title: '上游模型 ID', dataIndex: 'modelName', width: 180, render: (n: string) => <code>{n}</code> },
    {
      title: '上游 Provider', dataIndex: 'upstreamProvider', width: 120,
      render: (p: string) => <Tag color="geekblue">{p}</Tag>,
    },
    {
      title: 'Base URL', dataIndex: 'upstreamBaseUrl', width: 280,
      render: (u: string) => <Tooltip title={u}><code style={{ fontSize: 12 }}>{u}</code></Tooltip>,
    },
    {
      title: 'API Key',
      dataIndex: 'hasUpstreamApiKey',
      width: 180,
      render: (hasKey: boolean) => hasKey
        ? <Tag color="green">已配置（只写）</Tag>
        : <Tag color="red">未配置</Tag>,
    },
    {
      title: '单价 (元/1M)',
      width: 220,
      render: (_: any, r: ModelConfig) => (
        <div style={{ fontSize: 12, lineHeight: 1.5 }}>
          <div>输入 <b>¥{r.inputPricePerMTokens}</b> · 输出 <b>¥{r.outputPricePerMTokens}</b></div>
          <div style={{ color: '#52c41a' }}>缓存命中 ¥{r.cachedInputPricePerMTokens}</div>
        </div>
      ),
    },
    {
      title: '操作', width: 180, fixed: 'right' as const,
      render: (_: any, r: ModelConfig) => (
        <Space>
          <Button size="small" icon={<EditOutlined />} onClick={() => {
            setEditing(r);
            form.setFieldsValue({ ...r, upstreamApiKey: '' });
            setModalOpen(true);
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
        }}>新增模型</Button>
      </Space>

      <Table rowKey="id" loading={loading} dataSource={data} columns={columns} pagination={false} scroll={{ x: 1300 }} />

      <Modal
        title={editing ? `编辑模型 - ${editing.displayName}` : '新增模型'}
        open={modalOpen}
        onCancel={() => { setModalOpen(false); setEditing(null); }}
        onOk={() => form.submit()}
        width={700}
      >
        <Form form={form} layout="vertical" onFinish={onSubmit}
          initialValues={{
            upstreamProvider: 'openai',
            enabled: true,
            sortOrder: 10,
            inputPricePerMTokens: 1.0,
            outputPricePerMTokens: 2.0,
            cachedInputPricePerMTokens: 0.1,
            maxOutputTokens: 4096,
          }}>
          <Form.Item name="displayName" label="显示名" rules={[{ required: true }]}>
            <Input placeholder="DeepSeek-V3 · Pro" />
          </Form.Item>
          <Form.Item name="modelName" label="上游模型 ID" rules={[{ required: true }]}>
            <Input placeholder="deepseek-chat / gpt-4o-mini / claude-3-5-sonnet-..." />
          </Form.Item>
          <Form.Item name="upstreamProvider" label="上游协议" rules={[{ required: true }]}>
            <Select options={providerOptions} />
          </Form.Item>
          <Form.Item name="upstreamBaseUrl" label="上游 Base URL (无需带 /v1)" rules={[{ required: true }]}>
            <Input placeholder="https://api.deepseek.com" />
          </Form.Item>
          <Form.Item name="upstreamApiKey"
            label={editing ? 'API Key (留空 = 保留原值)' : 'API Key'}
            rules={editing ? [] : [{ required: true }]}>
            <Input.Password placeholder="sk-..." />
          </Form.Item>
          <Form.Item name="description" label="描述 (可选)">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.Item name="inputPricePerMTokens" label="输入单价 (元/1M tokens，未命中缓存)" rules={[{ required: true }]}>
            <InputNumber min={0} step={0.01} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item name="cachedInputPricePerMTokens" label="缓存命中输入单价 (元/1M tokens)" rules={[{ required: true }]} extra="上游返回 cached_tokens 时按此价计费，通常远低于未命中价">
            <InputNumber min={0} step={0.01} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item name="outputPricePerMTokens" label="输出单价 (元/1M tokens)" rules={[{ required: true }]}>
            <InputNumber min={0} step={0.01} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item name="maxOutputTokens" label="单次最大输出 Tokens" rules={[{ required: true }]} extra="用于限制上游输出并计算预授权点数；范围 1–131072">
            <InputNumber min={1} max={131072} step={256} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item name="sortOrder" label="排序 (数字越小越靠前)">
            <InputNumber min={0} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item name="enabled" label="启用" valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
