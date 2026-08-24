import { beforeEach, describe, expect, it } from 'vitest';
import { useConfirmDialogStore } from './confirmDialogStore';

describe('confirmDialogStore', () => {
  beforeEach(() => useConfirmDialogStore.setState({ request: null }));

  it('keeps the boolean ask API compatible', async () => {
    const result = useConfirmDialogStore.getState().ask({ title: 't', message: 'm' });
    useConfirmDialogStore.getState().close('confirm');
    await expect(result).resolves.toBe(true);
  });

  it('supports a three-way choice', async () => {
    const result = useConfirmDialogStore.getState().askChoice({
      title: 't', message: 'm', secondaryLabel: 'discard',
    });
    useConfirmDialogStore.getState().close('secondary');
    await expect(result).resolves.toBe('secondary');
  });

  it('cancels a concurrent hidden request instead of hanging it', async () => {
    void useConfirmDialogStore.getState().askChoice({ title: 'first', message: 'm' });
    await expect(useConfirmDialogStore.getState().askChoice({
      title: 'second', message: 'm',
    })).resolves.toBe('cancel');
    useConfirmDialogStore.getState().close('cancel');
  });
});
