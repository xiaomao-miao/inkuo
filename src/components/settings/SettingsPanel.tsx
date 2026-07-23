import { useState } from 'react';
import {
  Settings2,
  Cpu,
  Palette,
  Type,
  Keyboard,
  Sparkles,
  Info,
  AlertCircle,
  Brain,
  History,
  Globe,
  ChevronDown,
  ChevronRight,
} from 'lucide-react';
import { useSettingsStore, useInlineCompleteStore } from '../../store';
import { ModelsSettings, KnowledgeSettings, WebSearchSettings } from './index';
import { SnapshotsSettings } from './SnapshotsSettings';
import { AppearanceSettings } from './AppearanceSettings';
import { Select } from './Select';
import type { ExpertProfileName } from '../../types';
import styles from './SettingsPanel.module.css';

type SettingsTab =
  | 'models'
  | 'knowledge'
  | 'editor'
  | 'ai'
  | 'web_search'
  | 'snapshots'
  | 'appearance'
  | 'about';

export const SettingsPanel = () => {
  const settings = useSettingsStore((state) => state.settings);
  const updateSetting = useSettingsStore((state) => state.updateSetting);
  const setActiveApiConfig = useSettingsStore((state) => state.setActiveApiConfig);
  const enabled = useInlineCompleteStore((state) => state.enabled);
  const debounceMs = useInlineCompleteStore((state) => state.debounceMs);
  const setEnabled = useInlineCompleteStore((state) => state.setEnabled);
  const [activeTab, setActiveTab] = useState<SettingsTab>('models');

  const tabs: { id: SettingsTab; label: string; icon: React.ReactNode }[] = [
    { id: 'models', label: '模型', icon: <Cpu size={14} /> },
    { id: 'knowledge', label: '知识库', icon: <Brain size={14} /> },
    { id: 'editor', label: '编辑器', icon: <Type size={14} /> },
    { id: 'ai', label: 'AI', icon: <Sparkles size={14} /> },
    { id: 'web_search', label: '联网搜索', icon: <Globe size={14} /> },
    { id: 'snapshots', label: '快照', icon: <History size={14} /> },
    { id: 'appearance', label: '外观', icon: <Palette size={14} /> },
    { id: 'about', label: '关于', icon: <Info size={14} /> },
  ];

  const renderTabContent = () => {
    switch (activeTab) {
      case 'models':
        return <ModelsSettings />;
      case 'knowledge':
        return <KnowledgeSettings />;
      case 'editor':
        return (
          <div className={styles.tabContent}>
            <div className={styles.section}>
              <h4 className={styles.sectionTitle}>
                <Type size={14} />
                字体设置
              </h4>

              <div className={styles.field}>
                <label className={styles.label}>字体大小</label>
                <div className={styles.rangeWrapper}>
                  <input
                    type="range"
                    min="10"
                    max="24"
                    value={settings.editor_font_size}
                    onChange={(e) => {
                      void updateSetting('editor_font_size', parseInt(e.target.value));
                    }}
                    className={styles.range}
                  />
                  <span className={styles.rangeValue}>{settings.editor_font_size}px</span>
                </div>
              </div>

              <div className={styles.field}>
                <label className={styles.label}>字体</label>
                <Select
                  value={settings.editor_font_family}
                  options={[
                    { value: 'JetBrains Mono, monospace', label: 'JetBrains Mono' },
                    { value: 'Fira Code, monospace', label: 'Fira Code' },
                    { value: 'Cascadia Code, Consolas, monospace', label: 'Cascadia Code' },
                    { value: 'Consolas, monospace', label: 'Consolas' },
                    { value: 'Monaco, monospace', label: 'Monaco' },
                  ]}
                    onChange={(value) => {
                      void updateSetting('editor_font_family', value);
                    }}
                  className={styles.select}
                />
              </div>
            </div>

            <div className={styles.section}>
              <h4 className={styles.sectionTitle}>
                <Keyboard size={14} />
                编辑器选项
              </h4>

              <div className={styles.field}>
                <label className={styles.label}>自动换行</label>
                <div className={styles.toggleWrapper}>
                  <label className={styles.toggle}>
                    <input
                      type="checkbox"
                      checked={settings.editor_word_wrap}
                      onChange={(e) => {
                        void updateSetting('editor_word_wrap', e.target.checked);
                      }}
                    />
                    <span className={styles.toggleSlider}></span>
                  </label>
                  <span className={styles.toggleLabel}>{settings.editor_word_wrap ? '启用' : '禁用'}</span>
                </div>
              </div>

              <div className={styles.field}>
                <label className={styles.label}>显示行号</label>
                <div className={styles.toggleWrapper}>
                  <label className={styles.toggle}>
                    <input
                      type="checkbox"
                      checked={settings.editor_line_numbers}
                      onChange={(e) => {
                        void updateSetting('editor_line_numbers', e.target.checked);
                      }}
                    />
                    <span className={styles.toggleSlider}></span>
                  </label>
                  <span className={styles.toggleLabel}>{settings.editor_line_numbers ? '启用' : '禁用'}</span>
                </div>
              </div>
            </div>
          </div>
        );
      case 'ai': {
        const activeConfig = settings.apiConfigs.find(
          (c) => c.id === settings.activeApiConfigId
        );

        const modelOptions = settings.apiConfigs.map((config) => ({
          value: config.id,
          label: `${config.name} (${config.model})`,
        }));

        return (
          <div className={styles.tabContent}>
            <div className={styles.section}>
              <h4 className={styles.sectionTitle}>
                <Sparkles size={14} />
                AI Tab 补全
              </h4>
              <p className={styles.sectionDescription}>
                在编辑器中按 Tab 键触发 AI 代码补全建议。
              </p>

              <div className={styles.field}>
                <label className={styles.label}>AI 模型</label>
                {settings.apiConfigs.length === 0 ? (
                  <div className={styles.noConfigWarning}>
                    <AlertCircle size={14} />
                    <span>请先在「模型」设置中添加 API 配置</span>
                  </div>
                ) : (
                  <Select
                    value={settings.activeApiConfigId || ''}
                    options={modelOptions}
                    onChange={(value) => {
                      void setActiveApiConfig(value);
                    }}
                    className={styles.select}
                  />
                )}
                {activeConfig && (
                  <div className={styles.activeConfigInfo}>
                    <div className={styles.configDetail}>
                      <span className={styles.configLabel}>提供商:</span>
                      <span>{activeConfig.provider}</span>
                    </div>
                    <div className={styles.configDetail}>
                      <span className={styles.configLabel}>模型:</span>
                      <span>{activeConfig.model}</span>
                    </div>
                    <div className={styles.configDetail}>
                      <span className={styles.configLabel}>API URL:</span>
                      <span>{activeConfig.baseUrl}</span>
                    </div>
                  </div>
                )}
                {!activeConfig && settings.apiConfigs.length > 0 && (
                  <p className={styles.fieldHelp} style={{ color: 'var(--warning)' }}>
                    当前没有选中的 API 配置
                  </p>
                )}
              </div>

              <div className={styles.field}>
                <label className={styles.label}>启用 AI Tab 补全</label>
                <div className={styles.toggleWrapper}>
                  <label className={styles.toggle}>
                    <input
                      type="checkbox"
                      checked={enabled}
                      onChange={(e) => {
                        setEnabled(e.target.checked);
                      }}
                    />
                    <span className={styles.toggleSlider}></span>
                  </label>
                  <span className={styles.toggleLabel}>{enabled ? '启用' : '禁用'}</span>
                </div>
              </div>

              <div className={styles.field}>
                <label className={styles.label}>触发延迟</label>
                <div className={styles.rangeWrapper}>
                  <input
                    type="range"
                    min="100"
                    max="1000"
                    step="50"
                    value={debounceMs}
                    onChange={(e) => {
                      useInlineCompleteStore.getState().updateSettings({
                        debounceMs: parseInt(e.target.value)
                      });
                    }}
                    className={styles.range}
                  />
                  <span className={styles.rangeValue}>{debounceMs}ms</span>
                </div>
                <p className={styles.fieldHelp}>
                  按下 Tab 键后等待多久触发补全请求
                </p>
              </div>
            </div>

            <div className={styles.section}>
              <h4 className={styles.sectionTitle}>快捷键</h4>
              <div className={styles.shortcutList}>
                <div className={styles.shortcutItem}>
                  <kbd>Tab</kbd>
                  <span>触发/接受补全</span>
                </div>
                <div className={styles.shortcutItem}>
                  <kbd>Esc</kbd>
                  <span>拒绝补全</span>
                </div>
              </div>
            </div>

            <div className={styles.section}>
              <h4 className={styles.sectionTitle}>Agent 执行</h4>
              <p className={styles.sectionDescription}>
                Agent 模式下 LLM 与工具的多轮对话上限。达到上限后会返回
                「Maximum iterations reached」并中止。值越大单轮任务能做的
                步骤越多，但 token 与耗时也会显著上涨。
              </p>

              <div className={styles.field}>
                <label className={styles.label} htmlFor="agent-max-iterations">
                  工具调用循环上限
                </label>
                <div className={styles.rangeWrapper}>
                  <input
                    id="agent-max-iterations"
                    type="range"
                    min={1}
                    max={200}
                    step={1}
                    value={settings.agent_max_iterations}
                    onChange={(e) => {
                      void updateSetting(
                        'agent_max_iterations',
                        parseInt(e.target.value, 10)
                      );
                    }}
                    className={styles.range}
                  />
                  <span className={styles.rangeValue}>
                    {settings.agent_max_iterations} 次
                  </span>
                </div>
                <p className={styles.fieldHelp}>
                  范围 1–200，默认 50。仅影响主 Agent 会话；调用
                  <code>delegate_to</code> 派生的子 Agent 由下方的
                  「专家子 Agent 轮次上限」控制。
                </p>
              </div>
            </div>

            <div className={styles.section}>
              <h4 className={styles.sectionTitle}>专家子 Agent 轮次</h4>
              <p className={styles.sectionDescription}>
                主 Agent 在执行 Office / Markdown / 检索 / 批量 / 代码
                类任务时，会通过 <code>delegate_to</code> 委派给专门的
                子 Agent。每个子 Agent 都有独立的工具调用循环上限——
                默认全部为 50（已从之前的 10–25 提升）。需要时可在下面
                展开高级设置，单独调整每个专家的轮次。
              </p>

              <ExpertIterationsSettings />
            </div>
          </div>
        );
      }
      case 'appearance':
        return <AppearanceSettings />;
      case 'web_search':
        return <WebSearchSettings />;
      case 'snapshots':
        return <SnapshotsSettings />;
      case 'about':
        return (
          <div className={styles.tabContent}>
            <div className={styles.aboutSection}>
              <div className={styles.appLogo}>
                <Settings2 size={48} strokeWidth={1.5} />
              </div>
              <h2 className={styles.appName}>Inkuo</h2>
              <p className={styles.appVersion}>版本 1.0.0</p>
              <p className={styles.appDescription}>
                Inkuo 是一个 AI 文档编辑器，帮助你更高效地编辑和管理文档。
              </p>

              <div className={styles.aboutLinks}>
                <div className={styles.aboutLink}>
                  <span className={styles.linkLabel}>GitHub</span>
                  <span className={styles.linkValue}>github.com/inkuo/inkuo</span>
                </div>
                <div className={styles.aboutLink}>
                  <span className={styles.linkLabel}>文档</span>
                  <span className={styles.linkValue}>docs.inkuo.com</span>
                </div>
              </div>

              <div className={styles.techStack}>
                <h4>技术栈</h4>
                <div className={styles.techTags}>
                  <span className={styles.techTag}>Rust</span>
                  <span className={styles.techTag}>Tauri</span>
                  <span className={styles.techTag}>React</span>
                  <span className={styles.techTag}>TypeScript</span>
                  <span className={styles.techTag}>CodeMirror</span>
                </div>
              </div>
            </div>
          </div>
        );
      default:
        return null;
    }
  };

  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <h3>设置</h3>
      </div>

      <div className={styles.tabs}>
        {tabs.map((tab) => (
          <button
            key={tab.id}
            className={`${styles.tab} ${activeTab === tab.id ? styles.activeTab : ''}`}
            onClick={() => setActiveTab(tab.id)}
          >
            {tab.icon}
            <span>{tab.label}</span>
          </button>
        ))}
      </div>

      <div className={styles.content}>
        {renderTabContent()}
      </div>
    </div>
  );
};

/** UI labels for the per-expert sub-agent iteration cap, mirroring the
 * `label` field in `src-tauri/src/agent/prompts.rs::PROFILES`. */
const EXPERT_DISPLAY_INFO: Record<
  ExpertProfileName,
  { label: string; description: string }
> = {
  office_word_expert: {
    label: 'Word 文档专家',
    description: '创建 / 修改 .docx 文档',
  },
  office_excel_expert: {
    label: 'Excel 表格专家',
    description: '创建 / 修改 .xlsx 工作簿',
  },
  md_writer: {
    label: 'Markdown 写作',
    description: '长 Markdown 文档、README、设计文档',
  },
  researcher: {
    label: '检索专家',
    description: '只读搜索文件、定位内容',
  },
  batch_editor: {
    label: '批量编辑',
    description: '5+ 文件批量修改',
  },
  code_expert: {
    label: '代码工程专家',
    description: '代码 feature / 重构 / 修 bug',
  },
  flowchart_expert: {
    label: '流程图专家',
    description: '从 Mermaid / Markdown 渲染流程图 PNG/SVG',
  },
  word_image_expert: {
    label: 'Word 插图专家',
    description: '将本地图片插入到 .docx 文档',
  },
};

const EXPERT_ORDER: ExpertProfileName[] = [
  'office_word_expert',
  'office_excel_expert',
  'md_writer',
  'researcher',
  'batch_editor',
  'code_expert',
  'flowchart_expert',
  'word_image_expert',
];

const EXPERT_ITERATIONS_MIN = 1;
const EXPERT_ITERATIONS_MAX = 200;
const EXPERT_ITERATIONS_DEFAULT = 50;

/** Per-expert iteration cap settings panel.
 *
 * Renders one "default for all" slider (which writes the same value
 * into every expert) plus a collapsible "advanced" section with one
 * slider per expert. The advanced sliders write only that expert's
 * value, so the user can keep the unified default and tweak a single
 * expert if needed.
 */
const ExpertIterationsSettings = () => {
  const expertMaxIterations = useSettingsStore(
    (state) => state.settings.expert_max_iterations
  );
  const updateSetting = useSettingsStore(
    (state) => state.updateSetting
  );
  const [advancedOpen, setAdvancedOpen] = useState(false);

  /** True iff every expert has the same value (the slider can be
   *  shown as "linked"). When the user diverges any one expert, this
   *  becomes false and the unified slider moves to "—". */
  const uniqueValues = Array.from(
    new Set(EXPERT_ORDER.map((k) => expertMaxIterations[k]))
  );
  const isLinked = uniqueValues.length === 1;
  const linkedValue = isLinked ? uniqueValues[0] : null;

  const setAllExperts = (value: number) => {
    const next: Record<string, number> = {};
    for (const key of EXPERT_ORDER) {
      next[key] = value;
    }
    void updateSetting('expert_max_iterations', next);
  };

  const setExpert = (key: ExpertProfileName, value: number) => {
    void updateSetting('expert_max_iterations', {
      ...expertMaxIterations,
      [key]: value,
    });
  };

  return (
    <>
      <div className={styles.field}>
        <label className={styles.label} htmlFor="expert-default-iterations">
          所有专家统一上限
        </label>
        <div className={styles.rangeWrapper}>
          <input
            id="expert-default-iterations"
            type="range"
            min={EXPERT_ITERATIONS_MIN}
            max={EXPERT_ITERATIONS_MAX}
            step={1}
            value={linkedValue ?? EXPERT_ITERATIONS_DEFAULT}
            onChange={(e) => {
              const v = parseInt(e.target.value, 10);
              if (Number.isFinite(v)) setAllExperts(v);
            }}
            className={styles.range}
            disabled={!isLinked}
          />
          <span className={styles.rangeValue}>
            {isLinked ? `${linkedValue} 次` : '—  单独设置'}
          </span>
        </div>
        <p className={styles.fieldHelp}>
          {isLinked
            ? `范围 ${EXPERT_ITERATIONS_MIN}–${EXPERT_ITERATIONS_MAX}，默认 ${EXPERT_ITERATIONS_DEFAULT}。拖动此滑杆会同时更新下方所有专家的轮次。`
            : '已为个别专家设置了不同的轮次。如需恢复统一，可展开下方高级设置后逐个调整，或拖动本滑杆后所有专家会重置为同一值。'}
        </p>
      </div>

      <div className={styles.field}>
        <button
          type="button"
          onClick={() => setAdvancedOpen((v) => !v)}
          className={styles.advancedToggle}
          aria-expanded={advancedOpen}
        >
          {advancedOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          <span>高级设置：单独调整每个专家</span>
        </button>
        {advancedOpen && (
          <div className={styles.expertList}>
            {EXPERT_ORDER.map((key) => {
              const info = EXPERT_DISPLAY_INFO[key];
              const value = expertMaxIterations[key] ?? EXPERT_ITERATIONS_DEFAULT;
              return (
                <div key={key} className={styles.expertRow}>
                  <div className={styles.expertRowLabel}>
                    <span className={styles.expertRowName}>{info.label}</span>
                    <span className={styles.expertRowDesc}>{info.description}</span>
                  </div>
                  <div className={styles.rangeWrapper}>
                    <input
                      type="range"
                      min={EXPERT_ITERATIONS_MIN}
                      max={EXPERT_ITERATIONS_MAX}
                      step={1}
                      value={value}
                      onChange={(e) => {
                        const v = parseInt(e.target.value, 10);
                        if (Number.isFinite(v)) setExpert(key, v);
                      }}
                      className={styles.range}
                      aria-label={`${info.label}轮次上限`}
                    />
                    <span className={styles.rangeValue}>{value} 次</span>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </>
  );
};
