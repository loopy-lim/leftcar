import { cva } from "class-variance-authority";

export const inspectorButtonVariants = cva(
  "inline-flex items-center justify-center font-semibold transition-colors",
  {
    variants: {
      variant: {
        ghost: "border border-zinc-300 bg-transparent text-zinc-600 hover:bg-zinc-100",
      },
      size: {
        sm: "rounded-md px-2 py-1 text-[10px]",
      },
    },
    defaultVariants: {
      variant: "ghost",
      size: "sm",
    },
  },
);
