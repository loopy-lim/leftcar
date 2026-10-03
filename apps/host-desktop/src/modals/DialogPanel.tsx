import type { ReactNode } from "react";
import { X } from "lucide-react";
import { cn } from "@leftcar/ui-tokens";
import { Button } from "../ui/primitives";

export function DialogPanel({
  title,
  titleId,
  closeLabel,
  onClose,
  busy = false,
  size = "default",
  children,
}: {
  title: string;
  titleId?: string;
  closeLabel: string;
  onClose: () => void;
  busy?: boolean;
  size?: "compact" | "default" | "wide";
  children: ReactNode;
}) {
  return (
    <div
      className={cn(
        "flex max-h-[calc(100dvh-2rem)] w-full flex-col overflow-hidden rounded-lg border border-outline bg-canvas",
        size === "compact"
          ? "max-w-md"
          : size === "wide"
            ? "max-w-2xl"
            : "max-w-xl",
      )}
    >
      <div className="flex min-h-14 shrink-0 items-center justify-between gap-3 border-b border-outline bg-surface px-4">
        <h2 id={titleId} className="text-title text-ink font-semibold">
          {title}
        </h2>
        <Button
          variant="ghost"
          size="icon"
          disabled={busy}
          onClick={onClose}
          aria-label={closeLabel}
        >
          <X size={16} />
        </Button>
      </div>
      {children}
    </div>
  );
}
