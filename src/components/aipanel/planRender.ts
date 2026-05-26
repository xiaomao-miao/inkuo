export type PlanBlock = {
  title: string;
  lines: string[];
};

const KNOWN_TITLES = [
  'goal',
  'goals',
  'summary',
  'steps',
  'step',
  'risks',
  'tradeoffs',
  'files',
  'changes',
  'test plan',
  'test',
];

function normalizeTitle(raw: string) {
  return raw.trim().toLowerCase();
}

export function parsePlanBlocks(text: string): PlanBlock[] {
  const lines = text.split(/\r?\n/);

  const blocks: PlanBlock[] = [];
  let current: PlanBlock | null = null;

  const pushCurrent = () => {
    if (!current) return;
    // trim trailing empties
    while (current.lines.length > 0 && current.lines[current.lines.length - 1].trim() === '') {
      current.lines.pop();
    }
    if (current.lines.length > 0) blocks.push(current);
    current = null;
  };

  for (const line of lines) {
    const m = line.match(/^\s{0,3}(#{1,6})\s+(.+?)\s*$/);
    if (m) {
      const title = m[2];
      const norm = normalizeTitle(title);
      // treat any heading as a new block, but prefer known titles for stable grouping
      if (current) pushCurrent();
      current = { title: KNOWN_TITLES.includes(norm) ? title : title, lines: [] };
      continue;
    }

    if (!current) {
      // create implicit block
      current = { title: 'Plan', lines: [] };
    }

    current.lines.push(line);
  }

  pushCurrent();

  if (blocks.length === 0) {
    return [{ title: 'Plan', lines: [text] }];
  }

  return blocks;
}
