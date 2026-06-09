import { describe, expect, it } from 'vitest';
import { adjustColor } from './color';

describe('adjustColor', () => {
  it('keeps white unchanged when brightening', () => {
    expect(adjustColor('#ffffff', 20)).toBe('#ffffff');
  });

  it('brightens black', () => {
    expect(adjustColor('#000000', 20)).toBe('#333333');
  });

  it('darkens primary colors', () => {
    expect(adjustColor('#ff0000', -20)).toBe('#cc0000');
    expect(adjustColor('#0000ff', -20)).toBe('#0000cc');
  });

  it('supports short hex colors', () => {
    expect(adjustColor('#fff', 0)).toBe('#ffffff');
    expect(adjustColor('#123', 10)).toBe('#1a344d');
  });

  it('returns invalid input unchanged', () => {
    expect(adjustColor('invalid', 10)).toBe('invalid');
    expect(adjustColor('#XYZ', 0)).toBe('#XYZ');
  });
});
