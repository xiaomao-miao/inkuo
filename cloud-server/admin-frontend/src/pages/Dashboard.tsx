import { useCallback, useEffect, useRef, useState } from 'react';
import { Row, Col, Card, Statistic, Spin, Result, Button } from 'antd';
import {
  UserOutlined,
  CrownOutlined,
  DollarOutlined,
  ThunderboltOutlined,
  GiftOutlined,
  TagsOutlined,
  SearchOutlined,
  StopOutlined,
  ReloadOutlined,
} from '@ant-design/icons';
import ReactEChartsCore from 'echarts-for-react/lib/core';
import * as echarts from 'echarts/core';
import { BarChart, LineChart, PieChart } from 'echarts/charts';
import {
  GridComponent,
  LegendComponent,
  TitleComponent,
  TooltipComponent,
} from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import dayjs from 'dayjs';
import { dashboardApi, type DashboardSummary, type DailyUsagePoint, type PlanDistribution, type ModelUsageShare } from '../api/dashboard';

// Pull in only the chart primitives used on this page. Importing the ECharts
// all-in-one entry point made the lazy-loaded dashboard heavier than 1 MiB.
echarts.use([
  BarChart,
  LineChart,
  PieChart,
  GridComponent,
  LegendComponent,
  TitleComponent,
  TooltipComponent,
  CanvasRenderer,
]);

export default function DashboardPage() {
  const [summary, setSummary] = useState<DashboardSummary | null>(null);
  const [trend, setTrend] = useState<DailyUsagePoint[]>([]);
  const [planDist, setPlanDist] = useState<PlanDistribution[]>([]);
  const [modelUsage, setModelUsage] = useState<ModelUsageShare[]>([]);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const requestIdRef = useRef(0);

  const load = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setLoading(true);
    setFailed(false);
    try {
      const [s, t, p, m] = await Promise.all([
        dashboardApi.summary(),
        dashboardApi.usageTrend(),
        dashboardApi.planDistribution(),
        dashboardApi.modelUsage(),
      ]);
      if (requestId === requestIdRef.current) {
        setSummary(s);
        setTrend(t);
        setPlanDist(p);
        setModelUsage(m);
      }
    } catch {
      if (requestId === requestIdRef.current) setFailed(true);
    } finally {
      if (requestId === requestIdRef.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
    return () => { requestIdRef.current += 1; };
  }, [load]);

  if (loading) return <Spin size="large" tip="加载中..." style={{ display: 'block', textAlign: 'center', padding: 80 }} />;
  if (failed || !summary) {
    return (
      <Result
        status="warning"
        title="仪表盘加载失败"
        subTitle="请检查网络或服务状态后重试。"
        extra={<Button icon={<ReloadOutlined />} onClick={() => void load()}>重新加载</Button>}
      />
    );
  }

  const trendOption = {
    title: { text: '近 30 天对话 / 搜索用量', left: 'left' },
    tooltip: { trigger: 'axis' },
    legend: { data: ['对话消费', '搜索消费', 'Token 用量', '新用户'], top: 30 },
    grid: { top: 80, left: 60, right: 60, bottom: 40 },
    xAxis: { type: 'category', data: trend.map(d => dayjs(d.date).format('MM-DD')) },
    yAxis: [
      { type: 'value', name: '元 / 人', position: 'left' },
      { type: 'value', name: 'Tokens', position: 'right' },
    ],
    series: [
      {
        name: '对话消费',
        type: 'bar',
        stack: 'revenue',
        data: trend.map(d => +(d.chatCostPoints / 1000).toFixed(3)),
        itemStyle: { color: '#1677ff' },
        yAxisIndex: 0,
      },
      {
        name: '搜索消费',
        type: 'bar',
        stack: 'revenue',
        data: trend.map(d => +(d.webSearchCostPoints / 1000).toFixed(3)),
        itemStyle: { color: '#13c2c2' },
        yAxisIndex: 0,
      },
      {
        name: 'Token 用量',
        type: 'line',
        data: trend.map(d => d.tokens),
        smooth: true,
        itemStyle: { color: '#52c41a' },
        yAxisIndex: 1,
      },
      {
        name: '新用户',
        type: 'line',
        data: trend.map(d => d.newUsers),
        smooth: true,
        itemStyle: { color: '#faad14' },
        yAxisIndex: 0,
      },
    ],
  };

  const planOption = {
    title: { text: '套餐分布', left: 'left' },
    tooltip: { trigger: 'item' },
    series: [{
      type: 'pie',
      radius: '60%',
      data: planDist.map(p => ({ name: p.planName, value: p.subscriptions })),
    }],
  };

  const modelOption = {
    title: { text: '近 30 天模型用量 TOP', left: 'left' },
    tooltip: { trigger: 'axis', axisPointer: { type: 'shadow' } },
    grid: { top: 60, left: 100, right: 60, bottom: 40 },
    xAxis: { type: 'value' },
    yAxis: { type: 'category', data: modelUsage.map(m => m.modelName).reverse() },
    series: [{
      type: 'bar',
      data: modelUsage.map(m => m.tokens).reverse(),
      itemStyle: { color: '#722ed1' },
    }],
  };

  return (
    <div>
      <Row gutter={[16, 16]}>
        <Col xs={24} sm={12} md={6}>
          <Card>
            <Statistic
              title="总用户数"
              value={summary.totalUsers}
              prefix={<UserOutlined />}
              valueStyle={{ color: '#1677ff' }}
            />
            <div style={{ marginTop: 8, fontSize: 12, color: '#999' }}>
              本月新增 {summary.newUsersThisMonth}, 今日 {summary.newUsersToday}
            </div>
          </Card>
        </Col>
        <Col xs={24} sm={12} md={6}>
          <Card>
            <Statistic
              title="暂停账号"
              value={summary.suspendedUsers}
              prefix={<StopOutlined />}
              valueStyle={{ color: summary.suspendedUsers > 0 ? '#cf1322' : '#3f8600' }}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} md={6}>
          <Card>
            <Statistic
              title="活跃订阅"
              value={summary.activeSubscriptions}
              prefix={<CrownOutlined />}
              valueStyle={{ color: '#722ed1' }}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} md={6}>
          <Card>
            <Statistic
              title="本月收入 (元)"
              value={(summary.monthRevenuePoints / 1000).toFixed(3)}
              prefix={<DollarOutlined />}
              valueStyle={{ color: '#52c41a' }}
            />
            <div style={{ marginTop: 8, fontSize: 12, color: '#999' }}>
              对话 ¥{(summary.monthChatRevenuePoints / 1000).toFixed(3)} · 搜索 ¥{(summary.monthWebSearchRevenuePoints / 1000).toFixed(3)}
            </div>
            <div style={{ marginTop: 4, fontSize: 12, color: '#999' }}>
              累计 ¥{(summary.totalRevenuePoints / 1000).toFixed(3)}
            </div>
          </Card>
        </Col>
        <Col xs={24} sm={12} md={6}>
          <Card>
            <Statistic
              title="本月 Token 用量"
              value={summary.monthTokens}
              prefix={<ThunderboltOutlined />}
              valueStyle={{ color: '#faad14' }}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} md={6}>
          <Card>
            <Statistic
              title="本月 Web 搜索"
              value={summary.monthWebSearchRequests}
              prefix={<SearchOutlined />}
              valueStyle={{ color: '#13c2c2' }}
            />
            <div style={{ marginTop: 8, fontSize: 12, color: '#999' }}>
              累计 {summary.totalWebSearchRequests} 次
            </div>
          </Card>
        </Col>
        <Col xs={24} sm={12} md={6}>
          <Card>
            <Statistic
              title="邀请码使用率"
              value={summary.totalInviteCodes === 0 ? 0 : Math.round((summary.usedInviteCodes / summary.totalInviteCodes) * 100)}
              suffix="%"
              prefix={<GiftOutlined />}
            />
            <div style={{ marginTop: 8, fontSize: 12, color: '#999' }}>
              {summary.usedInviteCodes} / {summary.totalInviteCodes}
            </div>
          </Card>
        </Col>
        <Col xs={24} sm={12} md={6}>
          <Card>
            <Statistic
              title="兑换码使用率"
              value={summary.totalRedemptionCodes === 0 ? 0 : Math.round((summary.usedRedemptionCodes / summary.totalRedemptionCodes) * 100)}
              suffix="%"
              prefix={<TagsOutlined />}
            />
            <div style={{ marginTop: 8, fontSize: 12, color: '#999' }}>
              {summary.usedRedemptionCodes} / {summary.totalRedemptionCodes}
            </div>
          </Card>
        </Col>
      </Row>

      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        <Col xs={24} lg={16}>
          <Card>
            <ReactEChartsCore echarts={echarts} option={trendOption} notMerge lazyUpdate style={{ height: 360 }} />
          </Card>
        </Col>
        <Col xs={24} lg={8}>
          <Card>
            <ReactEChartsCore echarts={echarts} option={planOption} notMerge lazyUpdate style={{ height: 360 }} />
          </Card>
        </Col>
        <Col xs={24}>
          <Card>
            <ReactEChartsCore echarts={echarts} option={modelOption} notMerge lazyUpdate style={{ height: 320 }} />
          </Card>
        </Col>
      </Row>
    </div>
  );
}
