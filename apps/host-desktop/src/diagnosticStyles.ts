import { cva } from "class-variance-authority";

export const diagnosticValueVariants = cva("inspector-item-value", {
  variants: {
    tone: {
      default: "",
      active: "!text-sky-600",
      warning: "!text-amber-600",
    },
  },
  defaultVariants: {
    tone: "default",
  },
});
