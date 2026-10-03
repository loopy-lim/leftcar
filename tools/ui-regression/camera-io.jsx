import React, { useState, useCallback, useEffect } from 'react';
export * from './viewer-io.js';
const io = { permission: null, requests: [], settings: [], listeners: new Set(), focusEffects: new Set() };
window.cameraIo = io;
io.blur = () => io.focusEffects.forEach(entry => { entry.cleanup?.(); entry.cleanup = null; });
io.focus = () => io.focusEffects.forEach(entry => { if (!entry.cleanup) entry.cleanup = entry.effect(); });
export function useCameraPermissions() {
  const [permission, setPermission] = useState(io.permission);
  io.setPermission = setPermission;
  const read = useCallback(() => new Promise((resolve, reject) => io.requests.push({ resolve(value) { setPermission(value); resolve(value); }, reject })), []);
  return [permission, read, read];
}
export const AppState = { addEventListener(_event, listener) { io.listeners.add(listener); return { remove() { io.listeners.delete(listener); } }; } };
export const openSettings = () => new Promise((resolve, reject) => io.settings.push({ resolve, reject }));
export const useLocalSearchParams = () => globalThis.__viewerSearchParams ?? {};
export function useFocusEffect(effect) {
  useEffect(() => { const entry = { effect, cleanup: effect() }; io.focusEffects.add(entry); return () => { io.focusEffects.delete(entry); entry.cleanup?.(); }; }, [effect]);
}
export function useWindowDimensions() {
  const [size, set] = useState({ width: innerWidth, height: innerHeight });
  useEffect(() => { const update = () => set({ width: innerWidth, height: innerHeight }); addEventListener('resize', update); return () => removeEventListener('resize', update); }, []);
  return size;
}
export const useSafeAreaInsets = () => ({ top: 0, left: 0, right: 0, bottom: 0 });
export const useColorScheme = () => matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
const flatten = style => Array.isArray(style) ? Object.assign({}, ...style.map(flatten)) : style ?? {};
export const StyleSheet = { create: x => x, flatten };
function domStyle(style) {
  const input = flatten(typeof style === 'function' ? style({ pressed: false }) : style), output = {};
  for (const [key, value] of Object.entries(input)) {
    if (key === 'paddingHorizontal') { output.paddingLeft = value; output.paddingRight = value; }
    else if (key === 'paddingVertical') { output.paddingTop = value; output.paddingBottom = value; }
    else if (key === 'marginHorizontal') { output.marginLeft = value; output.marginRight = value; }
    else if (key === 'marginVertical') { output.marginTop = value; output.marginBottom = value; }
    else if (key === 'fontVariant') output.fontVariantNumeric = value.join(' ');
    else if (key !== 'elevation' && key !== 'includeFontPadding') output[key] = value;
  }
  return output;
}
const box = tag => React.forwardRef(function Box({ children, onPress, accessibilityLabel, accessibilityRole, accessibilityState, accessibilityLiveRegion, accessibilityValue, accessibilityElementsHidden, importantForAccessibility, nativeID, disabled, value, onChangeText, className, style, onFocus, onBlur, onSubmitEditing, keyboardType, placeholder, secureTextEntry, editable, pointerEvents, ..._rest }, ref) {
  return React.createElement(tag, { ref, id: nativeID, onClick: onPress, 'aria-label': accessibilityLabel, role: accessibilityRole,
    'aria-disabled': accessibilityState?.disabled, 'aria-busy': accessibilityState?.busy, 'aria-checked': accessibilityState?.checked, 'aria-selected': accessibilityState?.selected, 'aria-expanded': accessibilityState?.expanded,
    'aria-hidden': accessibilityElementsHidden || importantForAccessibility === 'no-hide-descendants' || undefined, 'aria-live': accessibilityLiveRegion,
    'aria-valuenow': accessibilityValue?.now, 'aria-valuemin': accessibilityValue?.min, 'aria-valuemax': accessibilityValue?.max,
    disabled: disabled || editable === false, value, className, style: { ...domStyle(style), ...(pointerEvents === 'none' ? { pointerEvents: 'none' } : {}) },
    'data-rn-view': tag === 'div' || tag === 'button' ? true : undefined, onFocus, onBlur,
    onKeyDown: onSubmitEditing ? e => { if (e.key === 'Enter') onSubmitEditing(); } : undefined,
    inputMode: keyboardType === 'number-pad' || keyboardType === 'numeric' ? 'numeric' : undefined, placeholder,
    type: tag === 'input' ? (secureTextEntry ? 'password' : 'text') : tag === 'button' ? 'button' : undefined,
    onChange: onChangeText ? e => onChangeText(e.target.value) : undefined }, tag === 'input' ? undefined : typeof children === 'function' ? children({ pressed: false }) : children);
});
export const View = box('div'), Text = box('span'), Pressable = box('button'), SafeAreaView = box('div'), TextInput = box('input');
export function ScrollView({ children, contentContainerClassName, contentContainerStyle, keyboardShouldPersistTaps, ...props }) {
  return <View {...props} style={[{ overflow: 'auto' }, props.style]}><View className={contentContainerClassName} style={contentContainerStyle}>{children}</View></View>;
}
const content = component => typeof component === 'function' ? React.createElement(component) : component;
export function FlatList({ data, renderItem, keyExtractor, ListHeaderComponent, ListFooterComponent, ListEmptyComponent, refreshControl, ...props }) {
  return <ScrollView {...props}>{content(ListHeaderComponent)}{data?.length ? data.map((item, index) => <React.Fragment key={keyExtractor?.(item, index) ?? index}>{renderItem({ item, index })}</React.Fragment>) : content(ListEmptyComponent)}{content(ListFooterComponent)}</ScrollView>;
}
export function Modal({ visible, children }) { return visible ? <div role="dialog" data-rn-view style={{ position: 'fixed', inset: 0, zIndex: 10 }}>{children}</div> : null; }
export const RefreshControl = () => null;
export const Switch = ({ value }) => <span aria-hidden="true">{value ? '●' : '○'}</span>;
export const ActivityIndicator = () => React.createElement('span', null, 'Loading');
export const Ionicons = () => null;
export const CameraView = props => React.createElement('div', { className: props.className, 'data-testid': 'live-camera' }, 'Controlled camera Surface');
