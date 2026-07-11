import { useEffect, useState } from 'react';
import {
  Table, Button, Space, Tag, Modal, Form, Input, Switch, App, Tooltip,
} from 'antd';
import {
  PlusOutlined, EditOutlined, DeleteOutlined, EyeOutlined,
} from '@ant-design/icons';
import { webSearchProvidersApi, WebSearchProvider } from '../api/webSearchProviders';

/**
 * Admin CRUD for web_search provider routing. Mirrors
 * `pages/ModelConfigs.tsx`: same Form/Table shape so a future operator
 * has the muscle memory; same "blank key on update = keep existing"
 * semantics so a stray save doesn't wipe the API key.
 *
 * Today the only provider is "baike" (Baidu Baike). The schema is
 * provider-agnostic so adding a future google/bing/tavily just means
 * inserting another row here — no code change.
 */
export default function WebSearchProvidersPage() {
  const [data, setData] = useState<WebSearchProvider[]>([]);
  const [loading, setLoading] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const [editing, setEditing] = useState<WebSearchProvider | null>(null);
  const [revealKey, setRevealKey] = useState(false);
  const [form] = Form.useForm();
  const { message, modal } = App.useApp();

  const load = async () => {
    setLoading(true);
    try {
      setData(await webSearchProvidersApi.list(revealKey));
    } finally {
      setLoading(false);
    }
  };
  useEffect(() => { load(); }, [revealKey]);

  const onSubmit = async (values: any) => {
    try {
      // Fall back to the editing record when an antd `disabled` Form.Item
      // drops a field from `values` (the field visually shows the old
      // value but `values.providerId` may be empty). The server treats
      // blank ProviderId as "keep existing" anyway, but doing this on
      // the client keeps the wire payload the UI actually shows.
      const src = { ...(editing ?? {}), ...values };
      const providerId = (src.providerId ?? '').toString().trim().toLowerCase();
      const displayName = (src.displayName ?? '').toString().trim();
      const upstreamBaseUrl = (src.upstreamBaseUrl ?? '').toString().trim() || null;
      const upstreamApiKey = (src.upstreamApiKey ?? '').toString().trim() || null;
      const enabled = !!values.enabled;

      // eslint-disable-next-line no-console
      console.log('[web-search-providers] submit', {
        values,
        editing,
        providerId,
        displayName,
        upstreamBaseUrl,
        upstreamApiKey,
        enabled,
      });

      if (!displayName) {
        message.warning('请填写显示名');
        return;
      }
      if (!editing && !upstreamApiKey) {
        message.warning('请先填写上游 API Key');
        return;
      }

      if (editing) {
        // Same pattern as model-configs: blank key on update = keep
        // existing. Drop the empty key on the wire so the server's
        // "leave existing if blank" branch fires.
        const updatePayload: any = { providerId, displayName, upstreamBaseUrl, enabled };
        if (upstreamApiKey) updatePayload.upstreamApiKey = upstreamApiKey;
        // eslint-disable-next-line no-console
        console.log('[web-search-providers] PUT payload=', updatePayload);
        await webSearchProvidersApi.update(editing.id, updatePayload);
        message.success('已更新');
      } else {
        await webSearchProvidersApi.create({
          providerId, displayName, upstreamBaseUrl, upstreamApiKey, enabled,
        });
        message.success('已创建');
      }
      setModalOpen(false);
      form.resetFields();
      setEditing(null);
      load();
    } catch (e: any) {
      message.error(e.response?.data?.error ?? '保存失败');
    }
  };

  const onDelete = (p: WebSearchProvider) => {
    modal.confirm({
      title: `删除「${p.displayName}」？`,
      content: '如果该 provider 已有调用记录,会被改为停用而非删除,以保留历史审计。',
      okType: 'danger',
      onOk: async () => {
        try {
          const r = await webSearchProvidersApi.delete(p.id);
          message.success(r.message);
          load();
        } catch (e: any) {
          message.error(e.response?.data?.error ?? '删除失败');
        }
      },
    });
  };

  const columns = [
    {
      title: 'Provider ID', dataIndex: 'providerId', width: 140,
      render: (v: string) => <Tag color="geekblue">{v}</Tag>,
    },
    {
      title: '显示名', dataIndex: 'displayName', width: 160,
      render: (n: string, r: WebSearchProvider) => (
        <Space>
          <strong>{n}</strong>
          <Tag color={r.enabled ? 'green' : 'default'}>{r.enabled ? '启用' : '停用'}</Tag>
        </Space>
      ),
    },
    {
      title: '上游 Base URL', dataIndex: 'upstreamBaseUrl', width: 320,
      render: (u: string | null) =>
        u ? <Tooltip title={u}><code style={{ fontSize: 12 }}>{u}</code></Tooltip> : <Tag>默认</Tag>,
    },
    {
      title: 'API Key', dataIndex: 'upstreamApiKeyMasked', width: 180,
      render: (k: string) => k ? <code>{k}</code> : <Tag color="red">未配置</Tag>,
    },
    {
      title: '创建时间', dataIndex: 'createdAt', width: 180,
      render: (v: string) => new Date(v).toLocaleString(),
    },
    {
      title: '操作', width: 160, fixed: 'right' as const,
      render: (_: any, r: WebSearchProvider) => (
        <Space>
          <Button size="small" icon={<EditOutlined />} onClick={() => {
            setEditing(r);
            form.setFieldsValue({
              providerId: r.providerId,
              displayName: r.displayName,
              upstreamBaseUrl: r.upstreamBaseUrl ?? '',
              upstreamApiKey: '',
              enabled: r.enabled,
            });
            setModalOpen(true);
          }}>编辑</Button>
          <Button size="small" danger icon={<DeleteOutlined />} onClick={() => onDelete(r)}>删除</Button>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <p style={{ marginBottom: 12, color: '#666' }}>
        桌面端 <code>web_search</code> 工具启用「走云端」后会调用云端本表配置的对应 provider,共用系统账户的 API Key,无需每个用户单独配。
      </p>
      <Space style={{ marginBottom: 16 }}>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => {
          setEditing(null);
          form.resetFields();
          form.setFieldsValue({ enabled: true });
          setModalOpen(true);
        }}>新增 Provider</Button>
        <Button icon={revealKey ? <EyeOutlined /> : undefined} onClick={() => setRevealKey(!revealKey)}>
          {revealKey ? '隐藏 API Key' : '查看完整 API Key'}
        </Button>
      </Space>

      <Table rowKey="id" loading={loading} dataSource={data} columns={columns} pagination={false} scroll={{ x: 1100 }} />

      <Modal
        title={editing ? `编辑 Provider - ${editing.displayName}` : '新增 Provider'}
        open={modalOpen}
        onCancel={() => { setModalOpen(false); setEditing(null); }}
        onOk={() => form.submit()}
        width={620}
      >
        <Form form={form} layout="vertical" onFinish={onSubmit}>
          <Form.Item name="providerId" label="Provider ID (内部 ID,英文/数字)" rules={[{ required: true, pattern: /^[a-z0-9_-]+$/, message: '仅允许小写字母、数字、下划线、短横线' }]} extra="桌面端通过该字段分发请求,例如 baike">
            <Input placeholder="baike" disabled={!!editing} />
          </Form.Item>
          <Form.Item name="displayName" label="显示名" rules={[{ required: true }]}>
            <Input placeholder="百度百科" />
          </Form.Item>
          <Form.Item name="upstreamBaseUrl" label="上游 Base URL (留空使用默认值)" extra="不填则按 Provider ID 选编译期默认值,如 baike = https://appbuilder.baidu.com/v2/baike/lemma/get_content">
            <Input placeholder="https://appbuilder.baidu.com/v2/baike/lemma/get_content" />
          </Form.Item>
          <Form.Item name="upstreamApiKey"
            label={editing ? 'API Key (留空 = 保留原值)' : 'API Key (Bearer Token)'}
            rules={editing ? [] : [{ required: true }]}>
            <Input.Password placeholder="appbuilder / sk-..." />
          </Form.Item>
          <Form.Item name="enabled" label="启用" valuePropName="checked">
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
