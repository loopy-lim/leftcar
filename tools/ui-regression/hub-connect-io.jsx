import { useEffect } from 'react';
export * from './host-io.jsx';
export const hubIo = { focusEffects: new Set() };
export function useFocusEffect(effect) {
  useEffect(() => {
    const entry = { effect, cleanup: effect() };
    hubIo.focusEffects.add(entry);
    return () => { hubIo.focusEffects.delete(entry); entry.cleanup?.(); };
  }, [effect]);
}
export function refocus() {
  for (const entry of hubIo.focusEffects) {
    entry.cleanup?.();
    entry.cleanup = entry.effect();
  }
}
