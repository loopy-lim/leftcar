/**
 * App-wide defaults that must stay importable from dependency-free leaf
 * modules (control.ts pulls react-native-tcp-socket and cannot serve as a
 * constants home for tests that mock or avoid it).
 */

/** Host control-plane port when an endpoint names no explicit port. */
export const DEFAULT_CONTROL_PORT = 7777;
