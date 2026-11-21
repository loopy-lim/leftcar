import type { ReactNode } from "react";
import { AlertTriangle } from "lucide-react";
import { cn } from "@leftcar/ui-tokens";
import { Text } from "./ui/primitives";
export function DiagnosticMetric({
  label,
  children,
  tone = "default",
}: {
  label: string;
  children: ReactNode;
  tone?: "default" | "warning" | "active";
}) {
  return (
    <div className="min-w-0 space-y-1">
      <Text variant="caption" tone="muted" className="block">
        {label}
      </Text>
      <Text
        variant="code"
        className={cn("block break-words", tone === "active" && "text-live")}
      >
        {tone === "warning" && (
          <AlertTriangle
            size={14}
            className="text-warning mr-1 inline-block align-text-bottom"
            aria-hidden="true"
          />
        )}
        {children}
      </Text>
    </div>
  );
}
