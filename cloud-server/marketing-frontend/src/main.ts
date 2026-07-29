import './styles.css';
import { Typewriter } from './typewriter';
import { fetchReleases, formatBytes, Release } from './releases';

// ---- inline SVG icons ----
const ICON_DOC = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="9" y1="13" x2="15" y2="13"/><line x1="9" y1="17" x2="15" y2="17"/></svg>`;
const ICON_CLOUD = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 10h-1.26A8 8 0 1 0 9 20h9a5 5 0 0 0 0-10z"/></svg>`;
const ICON_LOCAL = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="3"/><path d="M9 9h6v6H9z"/></svg>`;
const ICON_TABLE = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="3"/><line x1="3" y1="9" x2="21" y2="9"/><line x1="3" y1="15" x2="21" y2="15"/><line x1="9" y1="3" x2="9" y2="21"/><line x1="15" y1="3" x2="15" y2="21"/></svg>`;
const ICON_CODE = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/></svg>`;
const ICON_BOLT = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></svg>`;
const ICON_KEY = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4"/></svg>`;
const ICON_SEARCH = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>`;
const ICON_DL = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>`;
const ICON_COPY = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`;
const ICON_UP = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><polyline points="18 15 12 9 6 15"/></svg>`;

// ---- Feature data ----
const FEATURES = [
  { icon: ICON_DOC, title: 'Word / Excel / PPT / Markdown', desc: '你每天在用的文件,InkUO 都能直接打开、改、回写,不用换来换去。' },
  { icon: ICON_TABLE, title: '表格也能对话', desc: '直接问表里的问题,AI 帮你算、帮你改、帮你从中找出关键结论。' },
  { icon: ICON_SEARCH, title: '在一个文件夹里搜', desc: '不必翻遍子文件夹,直接问它:这堆资料里关于某某的内容在哪。' },
  { icon: ICON_CLOUD, title: '云端 AI,开箱即用', desc: '注册即送额度,DeepSeek、GPT 等主流模型按量付费,不必自己部署。' },
  { icon: ICON_LOCAL, title: '想自己跑模型也行', desc: '支持接入 Ollama 等本地模型服务,文件不出本机,AI 也照样能用。' },
  { icon: ICON_KEY, title: '邀请码注册', desc: '现在通过邀请码注册即送 ¥5 额度,先试到顺手再说。' },
];

// ---- Hero typewriter phrases ----
const TYPEWRITER_WORDS = [
  '让你专注想做什么',
  '帮你找资料',
  '帮你写方案',
  '帮你改报告',
  '帮你从表格里找结论',
];

// ---- DOM helpers ----
function h<K extends keyof HTMLElementTagNameMap>(
  tag: K, attrs: Record<string, string> = {}, children: (Node | string)[] = []
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === 'class') el.className = v;
    else if (k === 'html') el.innerHTML = v;
    else if (k === 'text') el.textContent = v;
    else if (k.startsWith('on') && typeof v === 'string') {
      (el as any)[k.toLowerCase()] = new Function('event', v);
    } else el.setAttribute(k, v);
  }
  for (const c of children) el.append(typeof c === 'string' ? document.createTextNode(c) : c);
  return el;
}

// ---- Page skeleton ----
function buildPage(): void {
  const root = document.getElementById('app');
  if (!root) throw new Error('#app not found');

  // ----- nav -----
  const nav = h('nav', { class: 'nav' }, [
    h('div', { class: 'container nav-inner' }, [
      h('a', { href: '#top', class: 'brand' }, [
        Object.assign(document.createElement('img'), { src: '/logo.svg', alt: 'inkuo' }),
        document.createTextNode('inkuo'),
      ]),
      h('div', { class: 'nav-links' }, [
        h('a', { href: '#features' }, ['特性']),
        h('a', { href: '#download' }, ['下载']),
        h('a', { href: '#changelog' }, ['更新日志']),
        h('a', { href: '/admin', class: 'nav-cta' }, ['登录 Admin']),
      ]),
    ]),
  ]);

  // ----- hero -----
  const twEl = h('span', { id: 'tw', class: 'tw-target' });
  const hero = h('section', { class: 'hero', id: 'top' }, [
    h('div', { class: 'container' }, [
      h('span', { class: 'hero-eyebrow' }, ['桌面端 · 中文优先 · 真正能改文件']),
      h('h1', { class: 'hero-title' }, ['让 AI 直接帮你处理文档']),
      h('div', { class: 'hero-type' }, [twEl, h('span', { class: 'hero-caret' })]),
      h('p', { class: 'hero-sub' }, [
        '打开一个文件夹,告诉 InkUO 你想做什么,它就去做:',
      ]),
      h('p', { class: 'hero-sub' }, [
        '找资料、写方案、改报告、从表格里找结论 —— 你说的,它来办。',
      ]),
      h('div', { class: 'hero-ctas' }, [
        h('a', { href: '#download', class: 'btn btn-primary', html: `<span style="display:inline-flex;align-items:center;gap:8px">${ICON_DL}立即下载</span>` }),
        h('a', { href: '#features', class: 'btn btn-ghost' }, ['了解更多']),
      ]),
      buildPreview(),
    ]),
  ]);

  // ----- features -----
  const featuresEl = h('div', { class: 'features' },
    FEATURES.map(f =>
      h('div', { class: 'feature reveal' }, [
        h('div', { class: 'feature-icon', html: f.icon }),
        h('h3', {}, [f.title]),
        h('p', {}, [f.desc]),
      ]),
    ),
  );
  const featuresSection = h('section', { id: 'features' }, [
    h('div', { class: 'container' }, [
      h('h2', { class: 'section-title reveal' }, ['把时间留给真正重要的事']),
      h('p', { class: 'section-sub reveal' }, ['InkUO 帮你处理文件,你只需要决定想做什么。']),
      featuresEl,
    ]),
  ]);

  // ----- download section (populated async) -----
  const downloadGrid = h('div', { class: 'downloads', id: 'download-grid' });
  const downloadSection = h('section', { id: 'download' }, [
    h('div', { class: 'container' }, [
      h('h2', { class: 'section-title reveal' }, ['下载安装包']),
      h('p', { class: 'section-sub reveal' }, ['当前所有启用的发行版。Latest 是我们推荐的主要版本。']),
      downloadGrid,
    ]),
  ]);

  // ----- footer -----
  const footer = h('footer', {}, [
    h('div', { class: 'container' }, [
      h('p', {}, [
        document.createTextNode('© 2026 inkuo team · '),
        Object.assign(document.createElement('a'), { href: 'https://github.com/inkuo/inkuo', textContent: 'GitHub', target: '_blank', rel: 'noreferrer' }),
        document.createTextNode(' · '),
        Object.assign(document.createElement('a'), { href: '/admin', textContent: '管理后台', target: '_blank', rel: 'noreferrer' }),
      ]),
    ]),
  ]);

  // ----- back-to-top -----
  const toTop = h('button', { class: 'to-top', 'aria-label': '回到顶部', html: ICON_UP });

  // assemble
  root.append(nav, hero, featuresSection, downloadSection, footer, toTop);

  // typewriter
  new Typewriter(twEl, TYPEWRITER_WORDS, { typeMs: 70, deleteMs: 35, holdMs: 1600, betweenMs: 500 }).start();

  // scroll reveal
  setupReveal();

  // back-to-top visibility
  window.addEventListener('scroll', () => {
    toTop.classList.toggle('visible', window.scrollY > 600);
  });
  toTop.addEventListener('click', () => window.scrollTo({ top: 0, behavior: 'smooth' }));

  // load releases
  void loadReleases(downloadGrid);
}

// ---- Build the editor preview mockup (CSS-only, no images) ----
function buildPreview(): HTMLElement {
  return h('div', { class: 'preview reveal' }, [
    h('div', { class: 'preview-bar' }, [
      h('div', { class: 'preview-dot r' }),
      h('div', { class: 'preview-dot y' }),
      h('div', { class: 'preview-dot g' }),
      h('div', { class: 'preview-name' }, ['inkuo · 季度会议 · 周报']),
    ]),
    h('div', { class: 'preview-body' }, [
      h('ul', { class: 'preview-side', style: 'list-style:none;padding:0;margin:0' }, [
        h('li', {}, ['📁 工作区']),
        h('li', { class: 'active' }, ['　📄 季度会议纪要']),
        h('li', {}, ['　📊 上季度销售汇总']),
        h('li', {}, ['　📄 产品反馈汇总']),
        h('li', {}, ['　📝 本周工作安排']),
        h('li', {}, ['📁 个人']),
      ]),
      h('div', { class: 'preview-doc' }, [
        h('h2', {}, ['季度会议纪要']),
        h('p', {}, [
          '时间:7 月 28 日 14:00  ·  与会:产品、运营、销售负责人', h('span', { class: 'cursor' }),
        ]),
        h('h3', {}, ['一、上季度回顾']),
        h('p', {}, ['销售收入环比增长 18%,新客户数同比持平,老客户续约率小幅下降。']),
        h('h3', {}, ['二、本季度重点']),
        h('p', {}, ['聚焦三件事:提升续约率、推进客户成功流程、迭代产品核心体验。']),
        h('h3', {}, ['三、待办与负责人']),
        h('p', {}, ['见文末表格。']),
      ]),
      h('div', { class: 'preview-ai' }, [
        h('div', { class: 'preview-msg user' }, ['把这份纪要润色一下,再补一段行动项']),
        h('div', { class: 'preview-msg' }, [
          h('strong', {}, ['inkuo AI']),
          h('p', { style: 'margin:6px 0 0;color:var(--fg-1);font-size:13px' }, [
            '已润色全文,语气更简洁。在第三节新增了表格,包含任务、负责人、截止时间。要我直接保存吗?',
          ]),
        ]),
      ]),
    ]),
  ]);
}

// ---- Releases rendering ----
async function loadReleases(grid: HTMLElement): Promise<void> {
  grid.innerHTML = '';
  grid.append(renderSkeleton());

  let releases: Release[];
  try {
    releases = await fetchReleases();
  } catch (err) {
    console.warn('release fetch failed', err);
    releases = [];
  }

  grid.innerHTML = '';
  if (releases.length === 0) {
    grid.append(
      h('div', { class: 'empty-state' }, [
        h('p', {}, [
          document.createTextNode('暂无发行版。管理员请到 '),
          Object.assign(document.createElement('code'), { textContent: '/admin' }),
          document.createTextNode(' → 发行版 页面上传安装包。'),
        ]),
      ]),
    );
    return;
  }

  for (const r of releases) {
    grid.append(renderReleaseCard(r));
  }
}

function renderReleaseCard(r: Release): HTMLElement {
  const dlBtn = h('a', {
    class: 'btn btn-primary',
    href: r.download_url,
    download: r.file_name,
    html: `<span style="display:inline-flex;align-items:center;gap:8px">${ICON_DL}下载 ${r.platform === 'windows' ? (r.architecture === 'aarch64' ? 'ARM64' : 'x64') : r.platform}</span>`,
  });
  const shaShort = `${r.sha256.slice(0, 10)}…${r.sha256.slice(-6)}`;
  const copyBtn = h('button', {
    class: 'btn-mini',
    title: '复制校验和',
    html: `<span style="display:inline-flex;align-items:center;gap:4px">${ICON_COPY}<code style="background:transparent;padding:0;font-size:12px">${shaShort}</code></span>`,
  });
  copyBtn.addEventListener('click', () => {
    const url = `${window.location.origin}${r.download_url}`;
    navigator.clipboard?.writeText(`${url}\nSHA-256: ${r.sha256}`).then(
      () => flashCopyBtn(copyBtn),
      () => { /* ignore */ },
    );
  });

  const notes = h('div', {
    class: 'dl-notes' + (r.release_notes ? '' : ' empty'),
    text: r.release_notes || '本次发布暂无更新日志。',
  });

  const created = new Date(r.created_at).toLocaleDateString('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit' });
  const metaHtml = `<span><strong>文件名:</strong> ${escapeHtml(r.file_name)}</span>`
    + `<span><strong>大小:</strong> ${escapeHtml(formatBytes(r.file_size_bytes))}</span>`
    + `<span><strong>发布于:</strong> ${escapeHtml(created)}</span>`;

  const card = h('div', { class: 'dl-card reveal' + (r.is_latest ? ' is-latest' : '') }, []);
  if (r.is_latest) card.append(h('span', { class: 'dl-tag' }, ['LATEST']));
  card.append(
    h('div', { class: 'dl-head' }, [
      h('span', { class: 'dl-version' }, [r.version]),
      h('span', { class: 'dl-channel ' + r.channel }, [r.channel]),
    ]),
    h('div', { class: 'dl-meta', html: metaHtml }),
    notes,
    h('div', { class: 'dl-actions' }, [dlBtn, copyBtn]),
  );
  return card;
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' } as Record<string, string>)[c]);
}

function renderSkeleton(): HTMLElement {
  return h('div', { class: 'dl-card', style: 'opacity:0.5' }, [
    h('div', { class: 'dl-head' }, [h('span', { class: 'dl-version' }, ['…'])]),
    h('div', { class: 'dl-meta' }, [h('span', {}, ['加载中…'])]),
  ]);
}

function flashCopyBtn(btn: HTMLElement): void {
  const original = btn.innerHTML;
  btn.innerHTML = '已复制 ✓';
  btn.setAttribute('style', 'background:rgba(25,219,227,0.2);color:var(--accent-strong)');
  setTimeout(() => { btn.innerHTML = original; btn.setAttribute('style', ''); }, 1200);
}

// ---- IntersectionObserver-based reveal ----
function setupReveal(): void {
  const els = document.querySelectorAll<HTMLElement>('.reveal');
  if (!('IntersectionObserver' in window)) {
    els.forEach(e => e.classList.add('visible'));
    return;
  }
  const io = new IntersectionObserver(entries => {
    for (const e of entries) {
      if (e.isIntersecting) {
        e.target.classList.add('visible');
        io.unobserve(e.target);
      }
    }
  }, { rootMargin: '0px 0px -10% 0px', threshold: 0.05 });
  els.forEach(e => io.observe(e));
}

// ---- go ----
if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', buildPage);
} else {
  buildPage();
}