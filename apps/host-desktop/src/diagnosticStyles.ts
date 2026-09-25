import { cva } from "class-variance-authority";

export const diagnosticValueVariants = cva("inspector-item-value", {
  variants: {
    tone: {
      default: "",
      active: "inspector-tone-active",
      warning: "inspector-tone-warning",
    },
  },
  defaultVariants: {
    tone: "default",
  },
});
