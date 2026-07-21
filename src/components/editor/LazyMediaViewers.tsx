// Lazy-loaded viewers for non-text file modes. Each is isolated in its own
// chunk so the editor doesn't pay the cost of pdf.js or image tooling
// until the user opens a tab that needs them.

import { lazy, Suspense } from 'react';

const ImageViewer = lazy(() =>
  import('./ImageViewer').then((m) => ({ default: m.ImageViewer })),
);
const SvgViewer = lazy(() =>
  import('./SvgViewer').then((m) => ({ default: m.SvgViewer })),
);
const PdfViewer = lazy(() =>
  import('./PdfViewer').then((m) => ({ default: m.PdfViewer })),
);

const RouteFallback = ({ label }: { label: string }) => (
  <div
    style={{
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      flex: 1,
      color: 'var(--fg-muted)',
      fontSize: 13,
      gap: 8,
    }}
  >
    {label}…
  </div>
);

export const LazyImageViewer: React.FC<{ filePath: string }> = (props) => (
  <Suspense fallback={<RouteFallback label="正在加载图片查看器" />}>
    <ImageViewer {...props} />
  </Suspense>
);

export const LazySvgViewer: React.FC<{ filePath: string }> = (props) => (
  <Suspense fallback={<RouteFallback label="正在加载 SVG 查看器" />}>
    <SvgViewer {...props} />
  </Suspense>
);

export const LazyPdfViewer: React.FC<{ filePath: string }> = (props) => (
  <Suspense fallback={<RouteFallback label="正在加载 PDF 查看器" />}>
    <PdfViewer {...props} />
  </Suspense>
);
