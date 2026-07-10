import { useEffect, useState } from 'react';
import { Table, Card, Statistic, Row, Col, DatePicker, Space, Button } from 'antd';
import { ReloadOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { usageApi, UsageRecord } from '../api/usage';

const { RangePicker } = DatePicker;

export default function UsagePage() {
  const [data, setData] = useState<UsageRecord[]>([]);
  const [total, setTotal] = useState(0);
  const [totalCostCents, setTotalCostCents] = useState(0);
  const [totalTokens, setTotalTokens] = useState(0);
  const [loading, setLoading] = useState(false);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(30);
  const [range, setRange] = useState<[dayjs.Dayjs, dayjs.Dayjs] | null>(null);

  const load = async () => {
    setLoading(true);
    try {
      const r = await usageApi.list({
        page, pageSize,
        from: range?.[0].toISOString(),
        to: range?.[1].toISOString(),
      });
      setData(r.items); setTotal(r.total);
      setTotalCostCents(r.totalCostCents); setTotalTokens(r.totalTokens);
    } finally { setLoading(false); }
  };
  useEffect(() => { load(); }, [page, pageSize, range]);

  const columns = [
    { title: '时间', dataIndex: 'recordedAt', width: 160, render: (d: string) => dayjs(d).format('YYYY-MM-DD HH:mm:ss') },
    { title: '用户', dataIndex: 'userEmail', width: 220, render: (e: string) => <code>{e}</code> },
    { title: '模型', dataIndex: 'modelName', width: 160 },
    { title: '输入 Tokens', dataIndex: 'promptTokens', width: 120, render: (t: number) => t.toLocaleString() },
    { title: '输出 Tokens', dataIndex: 'completionTokens', width: 120, render: (t: number) => t.toLocaleString() },
    { title: '费用 (元)', dataIndex: 'costCents', width: 120, render: (c: number) => `¥${(c / 100).toFixed(4)}` },
  ];

  return (
    <div>
      <Row gutter={[16, 16]} style={{ marginBottom: 16 }}>
        <Col xs={24} sm={8}>
          <Card>
            <Statistic title="总记录" value={total} />
          </Card>
        </Col>
        <Col xs={24} sm={8}>
          <Card>
            <Statistic title="总 Tokens" value={totalTokens.toLocaleString()} />
          </Card>
        </Col>
        <Col xs={24} sm={8}>
          <Card>
            <Statistic title="总消费 (元)" value={(totalCostCents / 100).toFixed(2)} valueStyle={{ color: '#1677ff' }} />
          </Card>
        </Col>
      </Row>

      <Space style={{ marginBottom: 16 }}>
        <RangePicker
          showTime
          value={range}
          onChange={(v) => { setRange(v as any); setPage(1); }}
        />
        <Button icon={<ReloadOutlined />} onClick={() => load()}>刷新</Button>
      </Space>

      <Table rowKey="id" loading={loading} dataSource={data} columns={columns}
        pagination={{ current: page, pageSize, total, onChange: setPage, showSizeChanger: true }} />
    </div>
  );
}