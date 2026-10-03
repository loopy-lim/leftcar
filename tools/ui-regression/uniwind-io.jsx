// Browser fixtures execute the real screens/primitives and compiled Tailwind CSS.
// Only the native style binding is substituted; Metro export separately checks
// the real Uniwind compiler and third-party component adapters.
export const withUniwind = Component => Component;
