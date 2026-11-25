import { split, View, type ViewProps } from "nsh";

const variants = {
  plain: "bg-transparent",
  card: "rounded-(--radius-lg) border border-outline bg-surface p-4",
  inset: "rounded-(--radius-md) border border-outline bg-subtle p-3",
} as const;

export interface SurfaceProps extends ViewProps {
  variant?: keyof typeof variants;
}

export function Surface(props: SurfaceProps) {
  const [, rest] = split(props, ["variant", "children"]);
  return (
    <View
      {...rest}
      class={`gap-3 ${variants[props.variant ?? "plain"]}${props.class ? ` ${props.class}` : ""}`}
    >
      {props.children}
    </View>
  );
}
