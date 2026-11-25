import { split, View, type ViewProps } from "nsh";

export interface NoticeProps extends ViewProps {
  tone?: "info" | "error";
}

export function Notice(props: NoticeProps) {
  const [, rest] = split(props, ["tone", "children"]);
  const error = () => props.tone === "error";
  return (
    <View
      {...rest}
      role={error() ? "alert" : props.role}
      android={{ accessibilityLiveRegion: error() ? "assertive" : "polite" }}
      class={`gap-2 rounded-(--radius-md) border bg-subtle p-3 ${error() ? "border-danger bg-danger-subtle" : "border-outline"}${props.class ? ` ${props.class}` : ""}`}
    >
      {props.children}
    </View>
  );
}
