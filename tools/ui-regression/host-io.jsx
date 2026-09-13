import React, { useEffect } from 'react';
export * from './camera-io.jsx';
export const hostIo = { navigations: [], blur: new Set(), handledTaps: null };
export const router = { push(value) { hostIo.navigations.push(value); }, replace(value) { hostIo.navigations.push(value); } };
export function useFocusEffect(effect) {
  useEffect(() => { const cleanup = effect(); hostIo.blur.add(cleanup); return () => { hostIo.blur.delete(cleanup); cleanup?.(); }; }, [effect]);
}
export const NativeEventEmitter = class { addListener() { return { remove() {} }; } };
// React Native's handled policy routes a child tap while the keyboard is open;
// the controlled DOM boundary records the actual caller's ScrollView policy.
export function ScrollView({ children, keyboardShouldPersistTaps }) {
  hostIo.handledTaps = keyboardShouldPersistTaps ?? 'never';
  return <div onClickCapture={event => {
    if (document.activeElement?.tagName === 'INPUT' && event.target.tagName === 'BUTTON' && hostIo.handledTaps === 'never') {
      document.activeElement.blur(); event.stopPropagation();
    }
  }}>{children}</div>;
}
