import { split, Text, type TextProps } from "nsh";

const variants = {
  caption: "text-(length:--text-caption) leading-(--leading-caption)",
  body: "text-(length:--text-body) leading-(--leading-body)",
  title: "text-(length:--text-title) leading-(--leading-title) font-semibold",
  heading: "text-(length:--text-heading) leading-(--leading-heading) font-semibold",
  code: "text-(length:--text-code) leading-(--leading-code) font-mono tabular-nums",
} as const;
const tones = { ink: "text-ink", muted: "text-muted" } as const;

export interface LabelProps extends TextProps {
  variant?: keyof typeof variants;
  tone?: keyof typeof tones;
}

export function Label(props: LabelProps) {
  const [, rest] = split(props, ["variant", "tone", "children"]);
  return (
    <Text
      {...rest}
      class={`${variants[props.variant ?? "body"]} ${tones[props.tone ?? "ink"]}${props.class ? ` ${props.class}` : ""}`}
    >
      {props.children}
    </Text>
  );
}
