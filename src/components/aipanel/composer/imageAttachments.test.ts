import { describe, expect, it } from 'vitest';

import {
  appendImagePaths,
  MAX_COMPOSER_IMAGE_ATTACHMENTS,
} from './imageAttachments';

describe('appendImagePaths', () => {
  it('creates high-detail path attachments and removes duplicates', () => {
    expect(appendImagePaths([], [
      '/workspace/page-001.png',
      '/workspace/page-001.png',
      'C:\\deck\\slide-02.JPG',
      '/workspace/not-an-image.txt',
    ])).toEqual([
      {
        path: '/workspace/page-001.png',
        detail: 'high',
        name: 'page-001.png',
      },
      {
        path: 'C:\\deck\\slide-02.JPG',
        detail: 'high',
        name: 'slide-02.JPG',
      },
    ]);
  });

  it('never exceeds the backend image-count ceiling', () => {
    const paths = Array.from({ length: 20 }, (_, index) => `/workspace/${index}.png`);
    expect(appendImagePaths([], paths)).toHaveLength(MAX_COMPOSER_IMAGE_ATTACHMENTS);
  });
});
