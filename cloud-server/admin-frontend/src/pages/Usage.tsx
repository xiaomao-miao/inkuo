import { useEffect, useState } from 'react';
import { Table, Card, Statistic, Row, Col, DatePicker, Space, Button, Segmented, Tag } from 'antd';
import { ReloadOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { usageApi, UsageRecord, UsageType } from '../api/usage';

const { RangePicker } = DatePicker;

export default function UsagePage() {
  const [data, setData] = useState<UsageRecord[]>([]);
  const [total, setTotal] = useState(0);
  const [totalCostPoints, setTotalCostPoints] = useState(0);
  const [totalTokens, setTotalTokens] = useState(0);
  const [chatRecords, setChatRecords] = useState(0);
  const [webSearchRecords, setWebSearchRecords] = useState(0);
  const [chatCostPoints, setChatCostPoints] = useState(0);
  const [webSearchCostPoints, setWebSearchCostPoints] = useState(0);
  const [loading, setLoading] = useState(false);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(30);
  const [range, setRange] = useState<[dayjs.Dayjs, dayjs.Dayjs] | null>(null);
  const [usageType, setUsageType] = useState<UsageType>('all');

  const load = async () => {
    setLoading(true);
    try {
      const r = await usageApi.list({
        page, pageSize,
        from: range?.[0].toISOString(),
        to: range?.[1].toISOString(),
        usageType,
      });
      setData(r.items); setTotal(r.total);
      setTotalCostPoints(r.totalCostPoints); setTotalTokens(r.totalTokens);
      setChatRecords(r.chatRecords); setWebSearchRecords(r.webSearchRecords);
      setChatCostPoints(r.chatCostPoints); setWebSearchCostPoints(r.webSearchCostPoints);
    } finally { setLoading(false); }
  };
  useEffect(() => { load(); }, [page, pageSize, range, usageType]);

  const statusColor = (status: string) => {
    if (status === 'settled' || status === 'estimated') return 'green';
    if (status === 'debt') return 'red';
    if (status === 'released') return 'default';
    return 'gold';
  };

  const columns = [
    {
      title: '类型', dataIndex: 'usageType', width: 90,
      render: (type: UsageRecord['usageType']) => (
        <Tag color={type === 'chat' ? 'purple' : 'cyan'}>{type === 'chat' ? '对话' : '搜索'}</Tag>
      ),
    },
    { title: '时间', dataIndex: 'recordedAt', width: 160, render: (d: string) => dayjs(d).format('YYYY-MM-DD HH:mm:ss') },
    { title: '用户', dataIndex: 'userEmail', width: 220, render: (e: string) => <code>{e}</code> },
    {
      title: '模型 / 搜索', key: 'resource', width: 260,
      render: (_: unknown, record: UsageRecord) => record.usageType === 'chat'
        ? record.modelName ?? '-'
        : (
          <div>
            <Tag color="cyan">{record.providerId ?? 'unknown'}</Tag>
            <span title={record.query ?? undefined}>
              {record.query && record.query.length > 32 ? `${record.query.slice(0, 32)}…` : record.query ?? '-'}
            </span>
          </div>
        ),
    },
    {
      title: '输入 Tokens', dataIndex: 'promptTokens', width: 120,
      render: (tokens: number, record: UsageRecord) => record.usageType === 'chat' ? tokens.toLocaleString() : '-',
    },
    {
      title: '输出 Tokens', dataIndex: 'completionTokens', width: 120,
      render: (tokens: number, record: UsageRecord) => record.usageType === 'chat' ? tokens.toLocaleString() : '-',
    },
    { title: '费用 (元)', dataIndex: 'costPoints', width: 120, render: (p: number) => `¥${(p / 1000).toFixed(3)}` },
    {
      title: '结算状态', dataIndex: 'billingStatus', width: 110,
      render: (status: string) => <Tag color={statusColor(status)}>{status}</Tag>,
    },
  ];

  return (
    <div>
      <Row gutter={[16, 16]} style={{ marginBottom: 16 }}>
        <Col xs={24} sm={8}>
          <Card>
            <Statistic title="总记录" value={total} />
            <div style={{ marginTop: 8, fontSize: 12, color: '#999' }}>
              对话 {chatRecords} · 搜索 {webSearchRecords}
            </div>
          </Card>
        </Col>
        <Col xs={24} sm={8}>
          <Card>
            <Statistic title="总 Tokens" value={totalTokens.toLocaleString()} />
          </Card>
        </Col>
        <Col xs={24} sm={8}>
          <Card>
            <Statistic title="总消费 (元)" value={(totalCostPoints / 1000).toFixed(3)} valueStyle={{ color: '#1677ff' }} />
            <div style={{ marginTop: 8, fontSize: 12, color: '#999' }}>
              对话 ¥{(chatCostPoints / 1000).toFixed(3)} · 搜索 ¥{(webSearchCostPoints / 1000).toFixed(3)}
            </div>
          </Card>
        </Col>
      </Row>

      <Space style={{ marginBottom: 16 }}>
        <Segmented
          value={usageType}
          options={[
            { label: '全部', value: 'all' },
            { label: '对话', value: 'chat' },
            { label: 'Web 搜索', value: 'search' },
          ]}
          onChange={(value) => { setUsageType(value as UsageType); setPage(1); }}
        />
        <RangePicker
          showTime
          value={range}
          onChange={(v) => { setRange(v as any); setPage(1); }}
        />
        <Button icon={<ReloadOutlined />} onClick={() => load()}>刷新</Button>
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
    </div>
  );
}
