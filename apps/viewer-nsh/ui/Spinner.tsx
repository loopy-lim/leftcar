import { View } from "nsh";

export function Spinner(props: { class?: string }) {
  return (
    <View
      role="progressbar"
      accessibilityState={{ busy: true }}
      class={`h-5 w-5 animate-spin self-center rounded-full border-2 border-muted border-t-transparent${props.class ? ` ${props.class}` : ""}`}
    />
  );
}
