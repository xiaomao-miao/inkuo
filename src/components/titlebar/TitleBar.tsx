import React, { useState, useRef, useEffect } from 'react';
import { 
  ChevronDown,
  X
} from 'lucide-react';
import { useSidebarStore, useEditorStore } from '../../store';
import { invoke } from '@tauri-apps/api/core';
import styles from './TitleBar.module.css';

interface MenuItem {
  label: string;
  shortcut?: string;
  action?: () => void;
  divider?: boolean;
  disabled?: boolean;
}

interface Menu {
  label: string;
  items: MenuItem[];
}

export const TitleBar: React.FC = () => {
  const [activeMenu, setActiveMenu] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  
  const { selectedFile, setWorkspacePath, setFiles } = useSidebarStore();
  const { documentContents, markSaved } = useEditorStore();
  
  const currentDoc = selectedFile ? documentContents[selectedFile] : null;
  const isDirty = currentDoc?.isDirty || false;

  // Close menu when clicking outside
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setActiveMenu(null);
      }
    };
    
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const loadDirectory = async (path: string) => {
    try {
      const { setIsLoading } = useSidebarStore.getState();
      setIsLoading(true);
      const entries = await invoke<any[]>('list_directory', { path });
      setFiles(entries);
      setIsLoading(false);
    } catch (err) {
      console.error('Failed to load directory:', err);
    }
  };

  const handleSave = async () => {
    if (!selectedFile || !isDirty) return;
    try {
      await invoke('write_document', {
        path: selectedFile,
        content: currentDoc?.content || '',
      });
      markSaved(selectedFile);
    } catch (err) {
      console.error('Failed to save:', err);
    }
    setActiveMenu(null);
  };

  const handleOpenFolder = async () => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: '选择工作区文件夹',
      });
      if (selected) {
        setWorkspacePath(selected);
        loadDirectory(selected);
      }
    } catch (err) {
      console.error('Failed to open folder:', err);
    }
    setActiveMenu(null);
  };

  const menus: Menu[] = [
    {
      label: '文件',
      items: [
        { label: '新建文件', shortcut: 'Ctrl+N', action: () => setActiveMenu(null) },
        { label: '打开文件夹...', shortcut: 'Ctrl+O', action: handleOpenFolder },
        { divider: true, label: '' },
        { label: '保存', shortcut: 'Ctrl+S', action: handleSave, disabled: !isDirty },
        { label: '另存为...', shortcut: 'Ctrl+Shift+S', action: () => setActiveMenu(null), disabled: !selectedFile },
        { divider: true, label: '' },
        { label: '关闭编辑器', shortcut: 'Ctrl+W', action: () => setActiveMenu(null), disabled: !selectedFile },
        { divider: true, label: '' },
        { label: '退出', shortcut: 'Alt+F4', action: () => window.close() },
      ],
    },
    {
      label: '编辑',
      items: [
        { label: '撤销', shortcut: 'Ctrl+Z', action: () => setActiveMenu(null) },
        { label: '重做', shortcut: 'Ctrl+Y', action: () => setActiveMenu(null) },
        { divider: true, label: '' },
        { label: '剪切', shortcut: 'Ctrl+X', action: () => setActiveMenu(null) },
        { label: '复制', shortcut: 'Ctrl+C', action: () => setActiveMenu(null) },
        { label: '粘贴', shortcut: 'Ctrl+V', action: () => setActiveMenu(null) },
        { divider: true, label: '' },
        { label: '全选', shortcut: 'Ctrl+A', action: () => setActiveMenu(null) },
        { label: '查找', shortcut: 'Ctrl+F', action: () => setActiveMenu(null) },
        { label: '替换', shortcut: 'Ctrl+H', action: () => setActiveMenu(null) },
      ],
    },
    {
      label: '选择',
      items: [
        { label: '全选', shortcut: 'Ctrl+A', action: () => setActiveMenu(null) },
        { label: '展开', shortcut: 'Ctrl+=', action: () => setActiveMenu(null) },
        { label: '收起', shortcut: 'Ctrl+-', action: () => setActiveMenu(null) },
      ],
    },
    {
      label: '视图',
      items: [
        { label: '侧边栏', shortcut: 'Ctrl+B', action: () => setActiveMenu(null) },
        { label: 'AI 面板', shortcut: 'Ctrl+J', action: () => setActiveMenu(null) },
        { divider: true, label: '' },
        { label: '放大字体', shortcut: 'Ctrl++', action: () => setActiveMenu(null) },
        { label: '缩小字体', shortcut: 'Ctrl+-', action: () => setActiveMenu(null) },
        { label: '重置字体大小', shortcut: 'Ctrl+0', action: () => setActiveMenu(null) },
        { divider: true, label: '' },
        { label: '全屏', shortcut: 'F11', action: () => setActiveMenu(null) },
      ],
    },
    {
      label: '帮助',
      items: [
        { label: '关于 inkuo', action: () => setActiveMenu(null) },
        { label: '快捷键参考', shortcut: 'Ctrl+K Ctrl+R', action: () => setActiveMenu(null) },
      ],
    },
  ];

  return (
    <div className={styles.titleBar} ref={menuRef}>
      <div className={styles.menuArea}>
        {menus.map(menu => (
          <div key={menu.label} className={styles.menuContainer}>
            <button
              className={`${styles.menuButton} ${activeMenu === menu.label ? styles.active : ''}`}
              onClick={() => setActiveMenu(activeMenu === menu.label ? null : menu.label)}
              onMouseEnter={() => activeMenu && setActiveMenu(menu.label)}
            >
              {menu.label}
            </button>
            {activeMenu === menu.label && (
              <div className={styles.dropdown}>
                {menu.items.map((item, index) => (
                  item.divider ? (
                    <div key={index} className={styles.divider} />
                  ) : (
                    <button
                      key={index}
                      className={`${styles.menuItem} ${item.disabled ? styles.disabled : ''}`}
                      onClick={() => !item.disabled && item.action?.()}
                      disabled={item.disabled}
                    >
                      <span className={styles.menuItemLabel}>{item.label}</span>
                      {item.shortcut && (
                        <span className={styles.menuItemShortcut}>{item.shortcut}</span>
                      )}
                    </button>
                  )
                ))}
              </div>
            )}
          </div>
        ))}
      </div>
      
      <div className={styles.title}>
        <span className={styles.appName}>inkuo</span>
        {selectedFile && (
          <>
            <span className={styles.separator}>—</span>
            <span className={styles.fileName}>
              {currentDoc?.document?.title || '未命名'}
              {isDirty && <span className={styles.dirty}>●</span>}
            </span>
          </>
        )}
      </div>
      
      <div className={styles.actions}>
        <button 
          className={styles.actionButton}
          onClick={() => setActiveMenu(null)}
          title="最小化"
        >
          <ChevronDown size={14} style={{ transform: 'rotate(180deg)' }} />
        </button>
        <button 
          className={`${styles.actionButton} ${styles.close}`}
          onClick={() => window.close()}
          title="关闭"
        >
          <X size={14} />
        </button>
      </div>
    </div>
  );
};
