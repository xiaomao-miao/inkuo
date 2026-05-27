import React, { useState } from 'react';
import { 
  FileText, 
  FilePlus, 
  Edit3, 
  FolderOpen, 
  Search, 
  Filter,
  ChevronDown,
  ChevronRight,
  BookOpen,
} from 'lucide-react';
import type { ChatMode } from './ModeSwitchSuggestion';
import styles from './ToolPanel.module.css';

interface Tool {
  name: string;
  description: string;
  icon?: React.ReactNode;
}

interface ToolGroup {
  title: string;
  icon: React.ReactNode;
  tools: Tool[];
}

const READ_ONLY_TOOLS: ToolGroup[] = [
  {
    title: '文件读取',
    icon: <FileText size={14} />,
    tools: [
      { name: 'read_file', description: '读取文件的完整内容，支持 offset 和 limit 参数' },
      { name: 'list_dir', description: '列出目录中的所有文件和子目录' },
      { name: 'glob', description: '根据 glob 模式查找文件 (如 **/*.rs, src/**/*.{ts,tsx})' },
      { name: 'grep', description: '在文件中搜索包含特定模式的行，支持正则表达式' },
    ],
  },
];

const FULL_TOOLS: ToolGroup[] = [
  {
    title: '文件读取',
    icon: <FileText size={14} />,
    tools: [
      { name: 'read_file', description: '读取文件的完整内容，支持 offset 和 limit 参数' },
      { name: 'list_dir', description: '列出目录中的所有文件和子目录' },
      { name: 'glob', description: '根据 glob 模式查找文件 (如 **/*.rs, src/**/*.{ts,tsx})' },
      { name: 'grep', description: '在文件中搜索包含特定模式的行，支持正则表达式' },
    ],
  },
  {
    title: '文件编辑',
    icon: <Edit3 size={14} />,
    tools: [
      { name: 'write_file', description: '创建新文件或覆盖现有文件' },
      { name: 'edit_file', description: '通过替换 old_text 为 new_text 来编辑文件' },
    ],
  },
];

const TOOL_ICONS: Record<string, React.ReactNode> = {
  read_file: <FileText size={14} />,
  list_dir: <FolderOpen size={14} />,
  glob: <Filter size={14} />,
  grep: <Search size={14} />,
  write_file: <FilePlus size={14} />,
  edit_file: <Edit3 size={14} />,
};

interface ToolPanelProps {
  mode: ChatMode;
  isExpanded?: boolean;
}

export const ToolPanel: React.FC<ToolPanelProps> = ({ mode, isExpanded: defaultExpanded = true }) => {
  const [isExpanded, setIsExpanded] = useState(defaultExpanded);
  const [filter, setFilter] = useState('');

  const toolGroups = mode === 'agent' ? FULL_TOOLS : READ_ONLY_TOOLS;

  const filteredGroups = toolGroups.map(group => ({
    ...group,
    tools: group.tools.filter(tool => 
      tool.name.toLowerCase().includes(filter.toLowerCase()) ||
      tool.description.toLowerCase().includes(filter.toLowerCase())
    ),
  })).filter(group => group.tools.length > 0);

  return (
    <div className={styles.container}>
      <button 
        className={styles.header}
        onClick={() => setIsExpanded(!isExpanded)}
      >
        {isExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        <BookOpen size={14} />
        <span className={styles.title}>可用工具</span>
        <span className={styles.badge}>
          {toolGroups.reduce((acc, g) => acc + g.tools.length, 0)}
        </span>
      </button>

      {isExpanded && (
        <>
          <div className={styles.search}>
            <Search size={12} className={styles.searchIcon} />
            <input
              type="text"
              placeholder="搜索工具..."
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              className={styles.searchInput}
            />
          </div>

          <div className={styles.content}>
            {filteredGroups.map((group) => (
              <div key={group.title} className={styles.group}>
                <div className={styles.groupHeader}>
                  {group.icon}
                  <span>{group.title}</span>
                </div>
                <div className={styles.toolList}>
                  {group.tools.map((tool) => (
                    <div key={tool.name} className={styles.tool}>
                      <div className={styles.toolHeader}>
                        {TOOL_ICONS[tool.name] || <FileText size={14} />}
                        <code className={styles.toolName}>{tool.name}</code>
                      </div>
                      <p className={styles.toolDesc}>{tool.description}</p>
                    </div>
                  ))}
                </div>
              </div>
            ))}

            {filteredGroups.length === 0 && (
              <div className={styles.empty}>没有找到匹配的工具</div>
            )}
          </div>
        </>
      )}
    </div>
  );
};
