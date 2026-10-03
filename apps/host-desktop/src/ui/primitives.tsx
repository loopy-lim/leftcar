import type { ButtonHTMLAttributes, HTMLAttributes, InputHTMLAttributes } from "react";
import {
  actionVariants, cn, inputVariants, noticeVariants, surfaceVariants, textVariants,
  type ActionVariantProps, type SurfaceVariantProps, type TextVariantProps,
} from "@leftcar/ui-tokens";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & ActionVariantProps & { busy?: boolean };
export function Button({ variant, size, busy = false, disabled = false, className, type = "button", ...props }: ButtonProps) {
  return <button {...props} type={type} disabled={disabled || busy} aria-busy={busy || undefined}
    className={cn(actionVariants({ variant, size, disabled: disabled || busy }), "text-body font-semibold focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus disabled:cursor-default", className)} />;
}

export function Field({ invalid = false, className, ...props }: InputHTMLAttributes<HTMLInputElement> & { invalid?: boolean }) {
  return <input {...props} aria-invalid={invalid || undefined}
    className={cn(inputVariants({ invalid }), "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus disabled:bg-subtle", className)} />;
}

export function Text({ variant, tone, className, ...props }: HTMLAttributes<HTMLSpanElement> & TextVariantProps) {
  return <span {...props} className={cn(textVariants({ variant, tone }), className)} />;
}

export function Surface({ variant, className, ...props }: HTMLAttributes<HTMLDivElement> & SurfaceVariantProps) {
  return <div {...props} className={cn(surfaceVariants({ variant }), className)} />;
}

export function Notice({ tone = "info", className, ...props }: HTMLAttributes<HTMLDivElement> & { tone?: "info" | "error" }) {
  return <div role={tone === "error" ? "alert" : "status"} {...props} className={cn(noticeVariants({ tone }), "text-body text-ink", className)} />;
}

export function Toggle({ checked, busy = false, disabled = false, className, ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { checked: boolean; busy?: boolean }) {
  return <button {...props} type="button" role="switch" aria-checked={checked} aria-busy={busy || undefined}
    disabled={disabled || busy}
    className={cn("flex h-11 min-w-11 items-center justify-center rounded-md focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus", className)}>
    <span aria-hidden="true" className={cn("flex h-6 w-10 items-center rounded-full border border-outline p-0.5", checked ? "bg-action justify-end" : "bg-subtle justify-start")}>
      <span className={cn("h-4 w-4 rounded-full", checked ? "bg-action-ink" : "bg-muted")} />
    </span>
  </button>;
}
