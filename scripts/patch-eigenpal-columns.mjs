import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const packageRoot = path.join(projectRoot, 'node_modules', '@eigenpal', 'docx-editor-core');
const packageJson = JSON.parse(fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'));

if (packageJson.version !== '1.9.0') {
  throw new Error(
    `Eigenpal column patch expects @eigenpal/docx-editor-core 1.9.0, found ${packageJson.version}. ` +
      'Review the upstream layout implementation before changing this guard.',
  );
}

const replacements = [
  {
    label: 'use the active column width for block fragments',
    before: 'getContentWidth:g',
    after: 'getContentWidth:()=>d',
  },
  {
    label: 'restore the natural page bottom when a continuous section changes columns',
    before:
      'function W(h){r=h,d=ee(t.w,n.left,n.right,r);let F=B();F.page.columns=r.count>1?{...r}:void 0,S=F.cursorY,F.columnIndex=0;}',
    after:
      'function W(h){r=h,d=ee(t.w,n.left,n.right,r);let F=B(),v=F.page.footnoteReservedHeight??0;F.contentBottom=u()-v,F.page.columns=r.count>1?{...r}:void 0,S=F.cursorY,F.columnIndex=0;}',
  },
  {
    label: 'balance every continuous multi-column section, not only the final one',
    before:
      'd[I+1]===void 0&&(L??"nextPage")==="continuous"&&(W.columns?.count??1)>1&&Qe({blocks:e,measures:t,paginator:M,start:w+1,end:e.length})',
    after:
      '(L??"nextPage")==="continuous"&&(W.columns?.count??1)>1&&Qe({blocks:e,measures:t,paginator:M,start:w+1,end:d[I+1]??e.length})',
  },
];

const distDir = path.join(packageRoot, 'dist');
const candidates = fs
  .readdirSync(distDir)
  .filter((name) => name.endsWith('.js') || name.endsWith('.mjs'))
  .map((name) => path.join(distDir, name));

let patchedFiles = 0;
for (const file of candidates) {
  let source = fs.readFileSync(file, 'utf8');
  let changed = false;
  for (const replacement of replacements) {
    if (source.includes(replacement.before)) {
      source = source.replace(replacement.before, replacement.after);
      changed = true;
    }
  }
  if (changed) {
    fs.writeFileSync(file, source);
    patchedFiles += 1;
  }
}

for (const replacement of replacements) {
  const matches = candidates.filter((file) =>
    fs.readFileSync(file, 'utf8').includes(replacement.after),
  );
  if (matches.length !== 2) {
    throw new Error(
      `Eigenpal patch invariant failed for "${replacement.label}": expected ESM and CJS matches, found ${matches.length}.`,
    );
  }
}

console.log(
  patchedFiles === 0
    ? 'Eigenpal column layout patch already applied.'
    : `Applied Eigenpal column layout patch to ${patchedFiles} files.`,
);
