import { useEffect, useState } from 'react';
import {
  Table, Button, Space, Tag, Modal, Form, Input, Switch, Select, App, Upload, Tooltip, Alert,
} from 'antd';
import {
  PlusOutlined, DeleteOutlined, CloudUploadOutlined, CopyOutlined, DownloadOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import { releasesApi, Release, UploadReleaseInput, formatBytes } from '../api/releases';

const { TextArea } = Input;

export default function ReleasesPage() {
  const [data, setData] = useState<Release[]>([]);
  const [loading, setLoading] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [form] = Form.useForm();
  const [pendingFile, setPendingFile] = useState<File | null>(null);
  const { message, modal } = App.useApp();

  const load = async () => {
    setLoading(true);
    try { setData(await releasesApi.list()); }
    finally { setLoading(false); }
  };
  useEffect(() => { load(); }, []);

  const openCreate = () => {
    form.resetFields();
    form.setFieldsValue({
      channel: 'stable', platform: 'windows', architecture: 'x86_64',
      isLatest: true, enabled: true,
    });
    setPendingFile(null);
    setModalOpen(true);
  };

  const onSubmit = async () => {
    try {
      const values = await form.validateFields();
      if (!pendingFile) {
        message.error('请选择安装包文件');
        return;
      }
      const payload: UploadReleaseInput = {
        version: values.version.trim(),
        channel: values.channel,
        platform: values.platform,
        architecture: values.architecture,
        releaseNotes: values.releaseNotes?.trim() || undefined,
        isLatest: !!values.isLatest,
        enabled: !!values.enabled,
        file: pendingFile,
      };
      setSubmitting(true);
      try {
        const r = await releasesApi.upload(payload);
        message.success(`已发布 ${r.version}（${formatBytes(r.fileSizeBytes)}）`);
        setModalOpen(false);
        load();
      } finally {
        setSubmitting(false);
      }
    } catch (e: any) {
      if (e?.errorFields) return; // form validation error
      message.error(e.response?.data?.error ?? '上传失败');
    }
  };

  const onDelete = (r: Release) => {
    modal.confirm({
      title: `删除 ${r.version}?`,
      content: '此操作会同时删除服务器上的安装包文件，不可撤销。',
      okType: 'danger',
      onOk: async () => {
        try { await releasesApi.remove(r.id); message.success('已删除'); load(); }
        catch (e: any) { message.error(e.response?.data?.error ?? '删除失败'); }
      },
    });
  };

  const onToggleEnabled = async (r: Release, enabled: boolean) => {
    try { await releasesApi.setEnabled(r.id, enabled); load(); }
    catch (e: any) { message.error(e.response?.data?.error ?? '更新失败'); }
  };

  const onToggleLatest = async (r: Release, isLatest: boolean) => {
    try { await releasesApi.setLatest(r.id, isLatest); load(); }
    catch (e: any) { message.error(e.response?.data?.error ?? '更新失败'); }
  };

  const copyDownloadUrl = (r: Release) => {
    const url = `${window.location.origin}${r.downloadUrl}`;
    navigator.clipboard?.writeText(url).then(
      () => message.success('下载链接已复制'),
      () => message.warning(`复制失败，可手动复制：${url}`),
    );
  };

  const columns = [
    {
      title: '版本', dataIndex: 'version', width: 130,
      render: (v: string, r: Release) => (
        <Space size={4}>
          <strong>{v}</strong>
          {r.isLatest && <Tag color="cyan">Latest</Tag>}
          {!r.enabled && <Tag>停用</Tag>}
        </Space>
      ),
    },
    {
      title: '平台 / 架构', width: 200,
      render: (_: any, r: Release) => (
        <span>
          <Tag color="geekblue">{r.platform}</Tag>
          <Tag>{r.architecture}</Tag>
          <Tag color={r.channel === 'beta' ? 'orange' : 'green'}>{r.channel}</Tag>
        </span>
      ),
    },
    {
      title: '文件名', dataIndex: 'fileName', ellipsis: true,
      render: (n: string) => <Tooltip title={n}><span>{n}</span></Tooltip>,
    },
    { title: '大小', dataIndex: 'fileSizeBytes', width: 100, render: formatBytes },
    {
      title: 'SHA-256', dataIndex: 'sha256', width: 130,
      render: (s: string) => (
        <Tooltip title={s}>
          <code style={{ fontSize: 12 }}>{s.slice(0, 10)}…{s.slice(-6)}</code>
        </Tooltip>
      ),
    },
    {
      title: '创建时间', dataIndex: 'createdAt', width: 160,
      render: (t: string) => dayjs(t).format('YYYY-MM-DD HH:mm'),
    },
    {
      title: '操作', width: 280, fixed: 'right' as const,
      render: (_: any, r: Release) => (
        <Space size={4}>
          <Tooltip title={r.enabled ? '点击停用' : '点击启用'}>
            <Switch size="small" checked={r.enabled} onChange={(v) => onToggleEnabled(r, v)} />
          </Tooltip>
          <Tooltip title={r.isLatest ? '已是 Latest' : '标记为 Latest'}>
            <Button
              size="small"
              type={r.isLatest ? 'primary' : 'default'}
              onClick={() => onToggleLatest(r, !r.isLatest)}
              disabled={!r.enabled}
            >
              {r.isLatest ? '★ Latest' : '设为 Latest'}
            </Button>
          </Tooltip>
          <Button size="small" icon={<CopyOutlined />} onClick={() => copyDownloadUrl(r)}>链接</Button>
          <Button size="small" icon={<DownloadOutlined />} href={r.downloadUrl} target="_blank">下载</Button>
          <Button size="small" danger icon={<DeleteOutlined />} onClick={() => onDelete(r)} />
        </Space>
      ),
    },
  ];

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
        <h2 style={{ margin: 0 }}>发行版管理</h2>
        <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>上传新版本</Button>
      </div>

      <Alert
        type="info" showIcon style={{ marginBottom: 16 }}
        message="用户从首页看到的是「启用」状态的所有发行版；建议每个 platform/architecture/channel 组合只保留一个 Latest，避免和首页大字推荐冲突。"
      />

      <Table<Release>
        rowKey="id" loading={loading} dataSource={data} columns={columns}
        pagination={{ pageSize: 20 }} scroll={{ x: 1100 }}
        locale={{ emptyText: '暂无发行版，点击右上角上传第一个版本' }}
      />

      <Modal
        title="上传新发行版"
        open={modalOpen}
        onCancel={() => setModalOpen(false)}
        onOk={onSubmit}
        confirmLoading={submitting}
        okText="上传"
        cancelText="取消"
        width={560}
        destroyOnClose
      >
        <Form form={form} layout="vertical" preserve={false}>
          <Form.Item
            name="version" label="版本号 (semver)"
            rules={[{ required: true, message: '请输入版本号' }, { max: 64 }]}
            extra="例: 0.1.0 / 1.2.3-beta.1"
          >
            <Input placeholder="0.1.0" />
          </Form.Item>

          <Space.Compact style={{ width: '100%' }}>
            <Form.Item name="channel" label="渠道" style={{ flex: 1, marginRight: 8 }}>
              <Select options={[
                { value: 'stable', label: 'stable (稳定)' },
                { value: 'beta', label: 'beta (测试)' },
              ]} />
            </Form.Item>
            <Form.Item name="platform" label="平台" style={{ flex: 1, marginRight: 8 }}>
              <Select options={[{ value: 'windows', label: 'Windows' }]} disabled />
            </Form.Item>
            <Form.Item name="architecture" label="架构" style={{ flex: 1 }}>
              <Select options={[
                { value: 'x86_64', label: 'x86_64' },
                { value: 'aarch64', label: 'aarch64' },
              ]} />
            </Form.Item>
          </Space.Compact>

          <Form.Item label="安装包文件" required>
            <Upload.Dragger
              beforeUpload={(file) => {
                setPendingFile(file);
                return false; // prevent auto-upload
              }}
              maxCount={1}
              onRemove={() => { setPendingFile(null); return true; }}
              fileList={pendingFile ? [{ uid: '-1', name: pendingFile.name, status: 'done' as const }] : []}
              accept=".exe,.msi,.zip,.dmg,.deb,.AppImage"
            >
              <p className="ant-upload-drag-icon"><CloudUploadOutlined /></p>
              <p className="ant-upload-text">点击或拖拽 .exe / .msi / 安装包到此区域</p>
              <p className="ant-upload-hint" style={{ fontSize: 12 }}>
                {pendingFile
                  ? <>已选择: <strong>{pendingFile.name}</strong> ({formatBytes(pendingFile.size)})</>
                  : '单文件最大 2 GiB；上传后会自动计算 SHA-256'}
              </p>
            </Upload.Dragger>
          </Form.Item>

          <Form.Item name="releaseNotes" label="更新日志 (Markdown, 可选)">
            <TextArea rows={4} placeholder={`## 新增\n- ...\n\n## 修复\n- ...`} />
          </Form.Item>

          <Space size={24}>
            <Form.Item name="isLatest" valuePropName="checked" noStyle>
              <Switch checkedChildren="Latest" unCheckedChildren="Latest" />
            </Form.Item>
            <Form.Item name="enabled" valuePropName="checked" noStyle>
              <Switch checkedChildren="启用" unCheckedChildren="停用" defaultChecked />
            </Form.Item>
          </Space>
        </Form>
      </Modal>
    </div>
  );
}