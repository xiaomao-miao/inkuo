import { useCallback, useEffect, useRef, useState } from 'react';
import {
  Table, Button, Input, Space, Tag, Drawer, Descriptions, App, Modal, Form, InputNumber,
  Statistic, Row, Col, Card, List, Empty, Tooltip, Popconfirm, type TableColumnsType,
  type TableProps,
} from 'antd';
import { SearchOutlined, ReloadOutlined, DollarOutlined, StopOutlined, DeleteOutlined, EyeOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { usersApi, type UserDetailResponse, type UserListItem } from '../api/users';
import { getApiErrorMessage } from '../api/client';
import { MAX_ADJUSTMENT_REASON_LENGTH, MAX_SINGLE_CREDIT_POINTS } from '../billingLimits';

export default function UsersPage() {
  const [data, setData] = useState<UserListItem[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(20);
  const [searchDraft, setSearchDraft] = useState('');
  const [searchQuery, setSearchQuery] = useState('');
  const [sortBy, setSortBy] = useState('createdAt');
  const [sortDir, setSortDir] = useState('desc');
  const [detail, setDetail] = useState<UserDetailResponse | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [adjustOpen, setAdjustOpen] = useState(false);
  const [adjustSaving, setAdjustSaving] = useState(false);
  const [adjustTarget, setAdjustTarget] = useState<UserListItem | null>(null);
  const [adjustForm] = Form.useForm();
  const { message, modal } = App.useApp();
  const requestIdRef = useRef(0);
  const detailRequestIdRef = useRef(0);

  const load = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setLoading(true);
    try {
      const result = await usersApi.list({ page, pageSize, search: searchQuery, sortBy, sortDir });
      if (requestId !== requestIdRef.current) return;
      setData(result.items);
      setTotal(result.total);
    } catch (error) {
      if (requestId === requestIdRef.current) {
        message.error(getApiErrorMessage(error, '加载用户列表失败'));
      }
    } finally {
      if (requestId === requestIdRef.current) setLoading(false);
    }
  }, [message, page, pageSize, searchQuery, sortBy, sortDir]);

  useEffect(() => {
    void load();
    return () => { requestIdRef.current += 1; };
  }, [load]);

  const openDetail = async (id: string) => {
    const requestId = ++detailRequestIdRef.current;
    setDetail(null);
    setDetailLoading(true);
    try {
      const d = await usersApi.detail(id);
      if (requestId !== detailRequestIdRef.current) return;
      setDetail(d);
    } catch (error) {
      if (requestId === detailRequestIdRef.current) {
        message.error(getApiErrorMessage(error, '加载用户详情失败'));
      }
    } finally {
      if (requestId === detailRequestIdRef.current) setDetailLoading(false);
    }
  };

  const onAdjust = async (values: { deltaPoints: number; reason: string }) => {
    if (!adjustTarget) return;
    setAdjustSaving(true);
    try {
      await usersApi.adjustBalance(adjustTarget.id, values.deltaPoints, values.reason.trim());
      message.success('余额已调整');
      setAdjustOpen(false);
      adjustForm.resetFields();
      await load();
    } catch (error) {
      message.error(getApiErrorMessage(error, '调整失败'));
    } finally {
      setAdjustSaving(false);
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
          if (data.length === 1 && page > 1) setPage(page - 1);
          else await load();
        } catch (error) {
          message.error(getApiErrorMessage(error, '删除失败'));
        }
      },
    });
  };

  const columns: TableColumnsType<UserListItem> = [
    {
      title: '邮箱', dataIndex: 'email', key: 'email',
      sorter: true, sortOrder: sortBy === 'email' ? (sortDir === 'asc' ? 'ascend' : 'descend') : null,
      render: (e: string) => <code>{e}</code>,
    },
    {
      title: '余额 (元)', dataIndex: 'balancePoints', key: 'balance', width: 110,
      sorter: true, sortOrder: sortBy === 'balance' ? (sortDir === 'asc' ? 'ascend' : 'descend') : null,
      render: (p: number) => <span style={{ color: p < 0 ? '#cf1322' : '#3f8600', fontWeight: 500 }}>{(p / 1000).toFixed(3)}</span>,
    },
    {
      title: '欠费 (元)', dataIndex: 'debtPoints', key: 'debtPoints', width: 110,
      render: (p: number, r: UserListItem) => p > 0
        ? <Tag color="red">¥{(p / 1000).toFixed(3)}{r.isSuspended ? ' · 已暂停' : ''}</Tag>
        : <Tag color="green">无</Tag>,
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
      title: '累计消费 (元)', dataIndex: 'totalCostPoints', key: 'totalCostPoints', width: 130,
      render: (p: number) => (p / 1000).toFixed(3),
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
          <Button size="small" icon={<EyeOutlined />} onClick={() => void openDetail(r.id)}>详情</Button>
          <Button size="small" icon={<DollarOutlined />} onClick={() => { setAdjustTarget(r); setAdjustOpen(true); }}>调账</Button>
          <Popconfirm title={`吊销 ${r.email} 的全部登录会话？`} onConfirm={() => onRevoke(r.id)}>
            <Button size="small" icon={<StopOutlined />} danger>吊销</Button>
          </Popconfirm>
          <Button size="small" icon={<DeleteOutlined />} danger onClick={() => onDelete(r)}>删除</Button>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <Space style={{ marginBottom: 16 }}>
        <Input.Search
          placeholder="搜索邮箱" prefix={<SearchOutlined />}
          value={searchDraft} onChange={e => setSearchDraft(e.target.value)}
          onSearch={(value) => {
            setSearchQuery(value.trim().toLowerCase());
            setPage(1);
          }}
          style={{ width: 280 }}
          allowClear
        />
        <Button icon={<ReloadOutlined />} loading={loading} onClick={() => void load()}>刷新</Button>
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
        onChange={((_, __, sorter) => {
          const activeSorter = Array.isArray(sorter) ? sorter[0] : sorter;
          if (activeSorter?.columnKey && activeSorter.order) {
            setSortBy(String(activeSorter.columnKey));
            setSortDir(activeSorter.order === 'ascend' ? 'asc' : 'desc');
            setPage(1);
          }
        }) satisfies NonNullable<TableProps<UserListItem>['onChange']>}
      />

      <Drawer
        title={detail ? `用户详情 - ${detail.user.email}` : '用户详情'}
        width={720}
        open={!!detail || detailLoading}
        onClose={() => {
          detailRequestIdRef.current += 1;
          setDetailLoading(false);
          setDetail(null);
        }}
        loading={detailLoading}
      >
        {detail && (
          <>
            <Row gutter={16} style={{ marginBottom: 16 }}>
              <Col span={8}>
                <Card>
                  <Statistic title="余额 (元)" value={(detail.user.balancePoints / 1000).toFixed(3)} valueStyle={{ color: '#1677ff' }} />
                </Card>
              </Col>
              <Col span={8}>
                <Card>
                  <Statistic title="欠费 (元)" value={(detail.user.debtPoints / 1000).toFixed(3)} valueStyle={{ color: detail.user.debtPoints > 0 ? '#cf1322' : '#3f8600' }} />
                </Card>
              </Col>
              <Col span={8}>
                <Card>
                  <Statistic title="累计消费 (元)" value={(detail.totalUsage.costPoints / 1000).toFixed(3)} />
                </Card>
              </Col>
            </Row>

            <Descriptions title="基础信息" bordered column={2} size="small">
              <Descriptions.Item label="用户 ID"><code>{detail.user.id}</code></Descriptions.Item>
              <Descriptions.Item label="邮箱">{detail.user.email}</Descriptions.Item>
              <Descriptions.Item label="注册时间">{dayjs(detail.user.createdAt).format('YYYY-MM-DD HH:mm:ss')}</Descriptions.Item>
              <Descriptions.Item label="使用邀请码">{detail.user.inviteCodeUsed || '-'}</Descriptions.Item>
              <Descriptions.Item label="账户状态">
                {detail.user.isSuspended ? <Tag color="red">已暂停</Tag> : <Tag color="green">正常</Tag>}
              </Descriptions.Item>
              <Descriptions.Item label="冻结点数">{detail.user.reservedPoints.toLocaleString()}</Descriptions.Item>
            </Descriptions>

            <Descriptions title="订阅历史" bordered column={2} size="small" style={{ marginTop: 16 }}>
              {detail.subscriptions.length === 0 && <Descriptions.Item>暂无订阅</Descriptions.Item>}
              {detail.subscriptions.map((s) => (
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
                renderItem={(u) => (
                  <List.Item>
                    <span><Tag>{u.modelName}</Tag></span>
                    <span>{u.promptTokens + u.completionTokens} tokens</span>
                    <span>¥{(u.costPoints / 1000).toFixed(3)}</span>
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
        onCancel={() => { setAdjustOpen(false); setAdjustTarget(null); adjustForm.resetFields(); }}
        onOk={() => adjustForm.submit()}
        confirmLoading={adjustSaving}
      >
        <Form form={adjustForm} layout="vertical" onFinish={onAdjust} initialValues={{ deltaPoints: 0 }}>
          <Form.Item name="deltaPoints" label="调整点数（可负数，1000 点 = ¥1）" rules={[
            { required: true },
            { validator: (_, value) => value === 0 ? Promise.reject(new Error('调整点数不能为 0')) : Promise.resolve() },
          ]}>
            <InputNumber
              min={-MAX_SINGLE_CREDIT_POINTS}
              max={MAX_SINGLE_CREDIT_POINTS}
              precision={0}
              style={{ width: '100%' }}
              step={1000}
            />
          </Form.Item>
          <Form.Item name="reason" label="原因 (会写入审计)" rules={[
            { required: true, whitespace: true, transform: (value?: string) => value?.trim() },
            { min: 3, max: MAX_ADJUSTMENT_REASON_LENGTH, transform: (value?: string) => value?.trim() },
          ]}>
            <Input.TextArea maxLength={MAX_ADJUSTMENT_REASON_LENGTH} showCount rows={3} placeholder="例如: 用户补偿 / 退款 / 测试" />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
