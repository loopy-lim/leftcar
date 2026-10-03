import { cva, type VariantProps } from "class-variance-authority";

/** Utilities compile on both Tailwind DOM and Uniwind native renderers. */
export const actionVariants = cva("flex flex-row items-center justify-center gap-2 rounded-md border border-transparent", {
  variants: {
    variant: {
      primary: "bg-action text-action-ink",
      secondary: "bg-subtle text-ink border-outline",
      ghost: "bg-transparent text-ink",
      danger: "bg-subtle text-ink border-strong",
    },
    size: {
      default: "min-h-12 min-w-11 px-4 py-3",
      compact: "min-h-11 min-w-11 px-3 py-2",
      icon: "h-11 w-11 p-2",
    },
    disabled: { true: "bg-subtle text-muted border-outline", false: "active:opacity-80" },
  },
  defaultVariants: { variant: "primary", size: "default", disabled: false },
});
export type ActionVariantProps = VariantProps<typeof actionVariants>;

export const actionLabelVariants = cva("text-body font-semibold text-center", {
  variants: {
    variant: { primary: "text-action-ink", secondary: "text-ink", ghost: "text-ink", danger: "text-ink" },
    disabled: { true: "text-muted", false: "" },
  },
  defaultVariants: { variant: "primary", disabled: false },
});

export const textVariants = cva("text-ink", {
  variants: {
    variant: {
      caption: "text-caption", body: "text-body", title: "text-title font-semibold",
      heading: "text-heading font-semibold", code: "text-code font-mono tabular-nums",
    },
    tone: { ink: "text-ink", muted: "text-muted" },
  },
  defaultVariants: { variant: "body", tone: "ink" },
});
export type TextVariantProps = VariantProps<typeof textVariants>;

export const surfaceVariants = cva("gap-3", {
  variants: { variant: {
    plain: "bg-transparent", card: "rounded-lg border border-outline bg-surface p-4",
    inset: "rounded-md border border-outline bg-subtle p-3",
  } },
  defaultVariants: { variant: "plain" },
});
export type SurfaceVariantProps = VariantProps<typeof surfaceVariants>;

export const inputVariants = cva("min-h-11 rounded-md border bg-surface px-3 py-2 text-body text-ink", {
  variants: { invalid: { true: "border-strong", false: "border-outline" } },
  defaultVariants: { invalid: false },
});

export const noticeVariants = cva("gap-2 rounded-md border bg-subtle p-3", {
  variants: { tone: { info: "border-outline", error: "border-strong" } },
  defaultVariants: { tone: "info" },
});
