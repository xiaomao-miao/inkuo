// Public surface of the floating AI popover system.
//
// `FloatingAiLayer` mounts all open popovers. Drop it once at the
// app root (inside a top-level layout container) and forget about it;
// the docx right-click menu takes care of spawning popovers via
// `useFloatingAiStore.open({...})`.

export { FloatingAiLayer, FloatingAiWindow } from './FloatingAiWindow';
export { useFloatingAiStream } from './useFloatingAiStream';
export type {
  FloatingAiWindow as FloatingAiWindowState,
  FloatingAiStatus,
} from '../../store/floatingAiStore';
