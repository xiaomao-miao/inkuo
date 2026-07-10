import { useEffect, useState } from 'react';
import {
  Table, Button, Input, Space, Tag, Drawer, Descriptions, App, Modal, Form, InputNumber,
  Statistic, Row, Col, Card, List, Empty, Tooltip, Popconfirm,
} from 'antd';
import { SearchOutlined, ReloadOutlined, DollarOutlined, StopOutlined, DeleteOutlined, EyeOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { usersApi, UserListItem } from '../api/users';

export default function UsersPage() {
  const [data, setData] = useState<UserListItem[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(20);
  const [search, setSearch] = useState('');
  const [sortBy, setSortBy] = useState('createdAt');
  const [sortDir, setSortDir] = useState('desc');
  const [detail, setDetail] = useState<any>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [adjustOpen, setAdjustOpen] = useState(false);
  const [adjustTarget, setAdjustTarget] = useState<UserListItem | null>(null);
  const [adjustForm] = Form.useForm();
  const { message, modal } = App.useApp();

  const load = async () => {
    setLoading(true);
    try {
      const result = await usersApi.list({ page, pageSize, search, sortBy, sortDir });
      setData(result.items);
      setTotal(result.total);
    } catch (e) {
      message.error('加载用户列表失败');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { load(); }, [page, pageSize, sortBy, sortDir]);

  const openDetail = async (id: string) => {
    setDetailLoading(true);
    try {
      const d = await usersApi.detail(id);
      setDetail(d);
    } finally {
      setDetailLoading(false);
    }
  };

  const onAdjust = async (values: { deltaCents: number; reason: string }) => {
    if (!adjustTarget) return;
    try {
      await usersApi.adjustBalance(adjustTarget.id, values.deltaCents, values.reason);
      message.success('余额已调整');
      setAdjustOpen(false);
      adjustForm.resetFields();
      load();
    } catch (e: any) {
      message.error(e.response?.data?.error ?? '调整失败');
    }
  };

  const onRevoke = async (id: string) => {
    try {
      const r = await usersApi.revokeSessions(id);
      message.success(`已吊销 ${r.revoked} 个会话`);
    } catch {
      message.error('吊销失败');
    }
  };

  const onDelete = (user: UserListItem) => {
    modal.confirm({
      title: `删除用户 ${user.email}？`,
      content: '将永久删除该用户的所有数据（订阅、用量记录、token）。此操作不可恢复！',
      okType: 'danger',
      okText: '确认删除',
      onOk: async () => {
        try {
          await usersApi.delete(user.id);
          message.success('用户已删除');
          load();
        } catch (e: any) {
          message.error(e.response?.data?.error ?? '删除失败');
        }
      },
    });
  };

  const columns = [
    {
      title: '邮箱', dataIndex: 'email', key: 'email',
      sorter: true, sortOrder: sortBy === 'email' ? (sortDir === 'asc' ? 'ascend' : 'descend') : null,
      render: (e: string) => <code>{e}</code>,
    },
    {
      title: '余额 (元)', dataIndex: 'balanceCents', key: 'balanceCents', width: 110,
      sorter: true, sortOrder: sortBy === 'balance' ? (sortDir === 'asc' ? 'ascend' : 'descend') : null,
      render: (c: number) => <span style={{ color: c < 0 ? '#cf1322' : '#3f8600', fontWeight: 500 }}>{(c / 100).toFixed(2)}</span>,
    },
    {
      title: '当前套餐', dataIndex: 'planName', key: 'planName', width: 120,
      render: (n: string | null, r: UserListItem) => n ? (
        <Tooltip title={r.subExpiresAt ? `到期 ${dayjs(r.subExpiresAt).format('YYYY-MM-DD')}` : ''}>
          <Tag color="purple">{n}</Tag>
        </Tooltip>
      ) : <Tag>无</Tag>,
    },
    {
      title: '累计 Tokens', dataIndex: 'totalTokens', key: 'totalTokens', width: 130,
      render: (t: number) => t.toLocaleString(),
    },
    {
      title: '累计消费 (元)', dataIndex: 'totalCostCents', key: 'totalCostCents', width: 130,
      render: (c: number) => (c / 100).toFixed(2),
    },
    {
      title: '注册时间', dataIndex: 'createdAt', key: 'createdAt', width: 160,
      sorter: true, sortOrder: sortBy === 'createdAt' ? (sortDir === 'asc' ? 'ascend' : 'descend') : null,
      render: (d: string) => dayjs(d).format('YYYY-MM-DD HH:mm'),
    },
    {
      title: '操作', key: 'action', width: 240, fixed: 'right' as const,
      render: (_: any, r: UserListItem) => (
        <Space size="small">
          <Button size="small" icon={<EyeOutlined />} onClick={() => openDetail(r.id)}>详情</Button>
          <Button size="small" icon={<DollarOutlined />} onClick={() => { setAdjustTarget(r); setAdjustOpen(true); }}>调账</Button>
          <Button size="small" icon={<StopOutlined />} danger onClick={() => onRevoke(r.id)}>吊销</Button>
          <Button size="small" icon={<DeleteOutlined />} danger onClick={() => onDelete(r)}>删除</Button>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <Space style={{ marginBottom: 16 }}>
        <Input
          placeholder="搜索邮箱" prefix={<SearchOutlined />}
          value={search} onChange={e => setSearch(e.target.value)}
          onPressEnter={() => { setPage(1); load(); }}
          style={{ width: 280 }}
          allowClear
        />
        <Button icon={<ReloadOutlined />} onClick={() => { setPage(1); load(); }}>刷新</Button>
      </Space>

      <Table
        rowKey="id"
        loading={loading}
        columns={columns}
        dataSource={data}
        scroll={{ x: 1200 }}
        pagination={{
          current: page, pageSize, total,
          showSizeChanger: true, showTotal: (t) => `共 ${t} 条`,
          onChange: (p, ps) => { setPage(p); setPageSize(ps); },
        }}
        onChange={(_, __, sorter: any) => {
          if (sorter.columnKey) {
            setSortBy(sorter.columnKey);
            setSortDir(sorter.order === 'ascend' ? 'asc' : 'desc');
          }
        }}
      />

      <Drawer
        title={detail ? `用户详情 - ${detail.user.email}` : '用户详情'}
        width={720}
        open={!!detail || detailLoading}
        onClose={() => setDetail(null)}
        loading={detailLoading}
      >
        {detail && (
          <>
            <Row gutter={16} style={{ marginBottom: 16 }}>
              <Col span={8}>
                <Card>
                  <Statistic title="余额 (元)" value={(detail.user.balanceCents / 100).toFixed(2)} valueStyle={{ color: '#1677ff' }} />
                </Card>
              </Col>
              <Col span={8}>
                <Card>
                  <Statistic title="累计 Tokens" value={detail.totalUsage.tokens.toLocaleString()} />
                </Card>
              </Col>
              <Col span={8}>
                <Card>
                  <Statistic title="累计消费 (元)" value={(detail.totalUsage.costCents / 100).toFixed(2)} />
                </Card>
              </Col>
            </Row>

            <Descriptions title="基础信息" bordered column={2} size="small">
              <Descriptions.Item label="用户 ID"><code>{detail.user.id}</code></Descriptions.Item>
              <Descriptions.Item label="邮箱">{detail.user.email}</Descriptions.Item>
              <Descriptions.Item label="注册时间">{dayjs(detail.user.createdAt).format('YYYY-MM-DD HH:mm:ss')}</Descriptions.Item>
              <Descriptions.Item label="使用邀请码">{detail.user.inviteCodeUsed || '-'}</Descriptions.Item>
            </Descriptions>

            <Descriptions title="订阅历史" bordered column={2} size="small" style={{ marginTop: 16 }}>
              {detail.subscriptions.length === 0 && <Descriptions.Item>暂无订阅</Descriptions.Item>}
              {detail.subscriptions.map((s: any) => (
                <Descriptions.Item key={s.id} label={s.planName} span={2}>
                  {dayjs(s.startedAt).format('YYYY-MM-DD')} 至 {dayjs(s.expiresAt).format('YYYY-MM-DD')} · <Tag color={s.status === 'active' ? 'green' : 'default'}>{s.status}</Tag>
                </Descriptions.Item>
              ))}
            </Descriptions>

            <h4 style={{ marginTop: 24 }}>最近用量记录 (最近 100 条)</h4>
            {detail.recentUsage.length === 0 ? (
              <Empty />
            ) : (
              <List
                size="small"
                dataSource={detail.recentUsage}
                renderItem={(u: any) => (
                  <List.Item>
                    <span><Tag>{u.modelName}</Tag></span>
                    <span>{u.promptTokens + u.completionTokens} tokens</span>
                    <span>¥{(u.costCents / 100).toFixed(4)}</span>
                    <span>{dayjs(u.recordedAt).format('MM-DD HH:mm:ss')}</span>
                  </List.Item>
                )}
              />
            )}
          </>
        )}
      </Drawer>

      <Modal
        title={adjustTarget ? `调整余额 - ${adjustTarget.email}` : ''}
        open={adjustOpen}
        onCancel={() => setAdjustOpen(false)}
        onOk={() => adjustForm.submit()}
      >
        <Form form={adjustForm} layout="vertical" onFinish={onAdjust} initialValues={{ deltaCents: 0 }}>
          <Form.Item name="deltaCents" label="调整金额 (分, 可负数, 1元=100分)" rules={[{ required: true }]}>
            <InputNumber style={{ width: '100%' }} step={100} />
          </Form.Item>
          <Form.Item name="reason" label="原因 (会写入审计)" rules={[{ required: true, min: 3 }]}>
            <Input.TextArea rows={3} placeholder="例如: 用户补偿 / 退款 / 测试" />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}