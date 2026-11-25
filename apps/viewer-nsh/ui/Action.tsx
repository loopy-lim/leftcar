import { split } from "nsh";
import { Button, type ButtonProps } from "nsh/base";
import { Label } from "./Label";
import { Spinner } from "./Spinner";

const variants = {
  primary: "bg-action text-action-ink",
  secondary: "bg-subtle text-ink border-outline",
  ghost: "bg-transparent text-ink",
  danger: "bg-danger-subtle text-ink border-danger",
} as const;
const labelTones = {
  primary: "text-action-ink",
  secondary: "text-ink",
  ghost: "text-ink",
  danger: "text-ink",
} as const;
const sizes = {
  default: "min-h-12 min-w-11 px-4 py-3",
  compact: "min-h-11 min-w-11 px-3 py-2",
  icon: "h-11 w-11 p-2",
} as const;

export interface ActionProps extends Omit<ButtonProps, "children"> {
  label: string;
  variant?: keyof typeof variants;
  size?: keyof typeof sizes;
  busy?: boolean;
  children?: ButtonProps["children"];
}

export function Action(props: ActionProps) {
  const [, rest] = split(props, ["label", "variant", "size", "busy", "disabled", "accessibilityLabel", "class", "children"]);
  const blocked = () => (props.disabled ?? false) || (props.busy ?? false);
  const variant = () => props.variant ?? "primary";
  return (
    <Button
      {...rest}
      disabled={blocked()}
      accessibilityLabel={props.accessibilityLabel ?? props.label}
      class={`flex-row items-center justify-center gap-2 rounded-(--radius-md) border border-transparent ${variants[variant()]} ${sizes[props.size ?? "default"]} ${blocked() ? "border-outline bg-subtle" : "active:opacity-80"}${props.class ? ` ${props.class}` : ""}`}
    >
      <If when={props.busy}>
        <Spinner />
      </If>
      <Label class={`shrink text-center font-semibold ${blocked() ? "text-muted" : labelTones[variant()]}`}>
        {props.label}
      </Label>
      {props.children}
    </Button>
  );
}
