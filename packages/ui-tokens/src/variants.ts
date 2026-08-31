import { cva, type VariantProps } from "class-variance-authority";

/**
 * Standardized Button Variants
 * Shared across Desktop and Mobile.
 */
export const buttonVariants = cva("btn-base", {
  variants: {
    variant: {
      primary: "btn-primary",
      ghost: "btn-ghost",
      danger: "btn-danger",
      icon: "btn-icon",
      close: "btn-close",
      link: "btn-link",
      stop: "btn-stop-stream",
    },
    size: {
      sm: "btn-sm",
      md: "btn-md",
      lg: "btn-lg",
    },
  },
  defaultVariants: {
    variant: "primary",
    size: "md",
  },
});

export type ButtonVariantProps = VariantProps<typeof buttonVariants>;

/**
 * Status Pill (Host/Viewer streaming status)
 */
export const statusPillVariants = cva("host-status-pill", {
  variants: {
    state: {
      active: "pill-active",
      idle: "pill-idle",
    },
  },
  defaultVariants: {
    state: "idle",
  },
});

export type StatusPillVariantProps = VariantProps<typeof statusPillVariants>;

/**
 * Control Toggle (Remote Mouse/Keyboard Toggle)
 */
export const controlToggleVariants = cva("btn-control-toggle", {
  variants: {
    active: {
      true: "toggle-active",
      false: "",
    },
  },
  defaultVariants: {
    active: false,
  },
});

export type ControlToggleVariantProps = VariantProps<typeof controlToggleVariants>;

/**
 * System and Network Alert Banners
 */
export const bannerAlertVariants = cva("banner-alert", {
  variants: {
    tone: {
      danger: "banner-danger",
      warning: "banner-warning",
      info: "banner-info",
    },
  },
  defaultVariants: {
    tone: "info",
  },
});

export type BannerAlertVariantProps = VariantProps<typeof bannerAlertVariants>;

/**
 * Termination Notice Banner
 */
export const terminationNoticeVariants = cva("termination-notice", {
  variants: {
    tone: {
      danger: "termination-danger",
      default: "termination-default",
      info: "termination-info",
    },
  },
  defaultVariants: {
    tone: "default",
  },
});

export type TerminationNoticeVariantProps = VariantProps<typeof terminationNoticeVariants>;

/**
 * Session Inspector Quick Action Buttons
 */
export const inspectorButtonVariants = cva("quality-auto-button", {
  variants: {
    active: {
      true: "button-active",
      false: "",
    },
  },
  defaultVariants: {
    active: false,
  },
});

export type InspectorButtonVariantProps = VariantProps<typeof inspectorButtonVariants>;
