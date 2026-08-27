import { useCallback, useEffect, useRef, useState } from 'react';
import {
  Table, Button, Space, Tag, Modal, Form, Input, InputNumber, Switch, DatePicker, Select, App, Popconfirm,
  type TableColumnsType,
} from 'antd';
import { PlusOutlined, EditOutlined, DeleteOutlined, CopyOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { redemptionCodesApi, type RedemptionCode } from '../api/redemptionCodes';
import { plansApi, type Plan } from '../api/plans';
import { getApiErrorMessage } from '../api/client';
import {
  CODE_PATTERN, MAX_CODE_LENGTH, MAX_CODE_USES, MAX_SINGLE_CREDIT_POINTS, MIN_CODE_LENGTH, trimCode,
} from '../billingLimits';

export default function RedemptionCodesPage() {
  const [data, setData] = useState<RedemptionCode[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(20);
  const [modalOpen, setModalOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [editing, setEditing] = useState<RedemptionCode | null>(null);
  const [plans, setPlans] = useState<Plan[]>([]);
  const [form] = Form.useForm();
  const { message } = App.useApp();
  const requestIdRef = useRef(0);

  const load = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setLoading(true);
    try {
      const r = await redemptionCodesApi.list(page, pageSize);
      if (requestId !== requestIdRef.current) return;
      setData(r.items); setTotal(r.total);
    } catch (error) {
      if (requestId === requestIdRef.current) {
        message.error(getApiErrorMessage(error, '加载兑换码失败'));
      }
    } finally {
      if (requestId === requestIdRef.current) setLoading(false);
    }
  }, [message, page, pageSize]);
  useEffect(() => {
    void load();
    return () => { requestIdRef.current += 1; };
  }, [load]);

  useEffect(() => {
    let cancelled = false;
    plansApi.list()
      .then((result) => { if (!cancelled) setPlans(result); })
      .catch((error) => {
        if (!cancelled) message.error(getApiErrorMessage(error, '加载套餐列表失败'));
      });
    return () => { cancelled = true; };
  }, [message]);

  const onSubmit = async (values: any) => {
    setSaving(true);
    try {
      const payload = {
        code: values.code.trim(),
        creditPoints: values.creditPoints ?? 0,
        planId: values.planId || null,
        maxUses: values.maxUses,
        expiresAt: values.expiresAt ? values.expiresAt.toISOString() : null,
        enabled: values.enabled,
      };
      if (editing) await redemptionCodesApi.update(editing.id, payload);
      else await redemptionCodesApi.create(payload);
      message.success(editing ? '兑换码已更新' : '兑换码已创建');
      setModalOpen(false); form.resetFields(); setEditing(null); await load();
    } catch (error) {
      message.error(getApiErrorMessage(error, '保存失败'));
    } finally {
      setSaving(false);
    }
  };

  const onToggle = async (id: number) => {
    try {
      await redemptionCodesApi.toggle(id); message.success('状态已切换'); await load();
    } catch (error) { message.error(getApiErrorMessage(error, '操作失败')); }
  };

  const onDelete = async (id: number) => {
    try {
      await redemptionCodesApi.delete(id); message.success('已删除'); await load();
    } catch (error) { message.error(getApiErrorMessage(error, '删除失败')); }
  };

  const copy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      message.success(`已复制: ${text}`);
    } catch {
      message.error('复制失败，请手动选择代码');
    }
  };

  const columns: TableColumnsType<RedemptionCode> = [
    {
      title: '代码', dataIndex: 'code', width: 220,
      render: (c: string) => (
        <Space>
          <code style={{ fontSize: 14, fontWeight: 600 }}>{c}</code>
          <Button aria-label={`复制兑换码 ${c}`} size="small" type="text" icon={<CopyOutlined />} onClick={() => void copy(c)} />
        </Space>
      ),
    },
    {
      title: '类型',
      width: 120,
      render: (_: any, r: RedemptionCode) => r.planId
        ? <Tag color="purple">套餐开通: {r.planName}</Tag>
        : <Tag color="blue">充值 ¥{(r.creditPoints / 1000).toFixed(3)}</Tag>,
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
          <Button type="text" size="small" aria-label={`${e ? '禁用' : '启用'}兑换码 ${r.code}`}>
            <Tag color={e ? 'green' : 'default'}>{e ? '启用' : '停用'}</Tag>
          </Button>
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
          <Popconfirm title={`确定删除兑换码 ${r.code}？`} okText="删除" okButtonProps={{ danger: true }} onConfirm={() => onDelete(r.id)}>
            <Button size="small" danger icon={<DeleteOutlined />}>删除</Button>
          </Popconfirm>
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

      <Modal
        title={editing ? `编辑兑换码 - ${editing.code}` : '新增兑换码'}
        open={modalOpen}
        onCancel={() => { setModalOpen(false); setEditing(null); }}
        onOk={() => form.submit()}
        confirmLoading={saving}
        width={600}
      >
        <Form form={form} layout="vertical" onFinish={onSubmit}
          initialValues={{ maxUses: 1, enabled: true, creditPoints: 0 }}>
          <Form.Item name="code" label="兑换码 (建议大写)" rules={[
            { required: true, whitespace: true, transform: trimCode },
            { min: MIN_CODE_LENGTH, max: MAX_CODE_LENGTH, transform: trimCode },
            { pattern: CODE_PATTERN, message: '仅可使用英文字母、数字、- 和 _', transform: trimCode },
          ]}>
            <Input maxLength={MAX_CODE_LENGTH} placeholder="PLUS-MAR2026" style={{ textTransform: 'uppercase' }} />
          </Form.Item>
          <Form.Item name="planId" label="绑定套餐 (留空 = 纯充值)">
            <Select allowClear options={planOptions} placeholder="选择套餐则开通一个月, 否则只充值余额" />
          </Form.Item>
          <Form.Item name="creditPoints" label="充值点数（1000 点 = ¥1，套餐模式可为 0）">
            <InputNumber min={0} max={MAX_SINGLE_CREDIT_POINTS} precision={0} style={{ width: '100%' }} step={1000} />
          </Form.Item>
          <Form.Item name="maxUses" label="最大使用次数" rules={[{ required: true }]}>
            <InputNumber min={Math.max(1, editing?.usedCount ?? 0)} max={MAX_CODE_USES} precision={0} style={{ width: '100%' }} />
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
