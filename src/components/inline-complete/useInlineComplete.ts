import { useContext } from 'react';
import { InlineCompleteContext, type InlineCompleteContextValue } from './inlineCompleteContext';

export function useInlineComplete(): InlineCompleteContextValue {
  const context = useContext(InlineCompleteContext);
  if (!context) {
    throw new Error('useInlineComplete must be used within InlineCompleteProvider');
  }
  return context;
}
