import { forwardRef, type ComponentProps } from "react";
import {
  ActivityIndicator,
  Pressable,
  Text,
  TextInput,
  View,
  type PressableProps,
  type TextInputProps,
  type TextProps,
  type ViewProps,
} from "react-native";
import {
  actionLabelVariants,
  actionVariants,
  cn,
  inputVariants,
  noticeVariants,
  surfaceVariants,
  textVariants,
  type ActionVariantProps,
  type SurfaceVariantProps,
  type TextVariantProps,
} from "@leftcar/ui-tokens";
import { useAppTheme } from "../theme";
import { SafeAreaView } from "react-native-safe-area-context";
import { withUniwind } from "uniwind";

// The Fabric safe-area component is not a core React Native style target.
const StyledSafeArea = withUniwind(SafeAreaView);
export function SafeArea(props: ComponentProps<typeof StyledSafeArea>) {
  return <StyledSafeArea {...props} />;
}

type ActionProps = PressableProps &
  ActionVariantProps & { label: string; className?: string; busy?: boolean };
export function Action({
  label,
  variant,
  size,
  busy = false,
  disabled = false,
  accessibilityState,
  className,
  children,
  ...props
}: ActionProps) {
  const { colors } = useAppTheme();
  const blocked = disabled || busy;
  return (
    <Pressable
      {...props}
      accessibilityRole={props.accessibilityRole ?? "button"}
      accessibilityLabel={props.accessibilityLabel ?? label}
      accessibilityState={{ ...accessibilityState, disabled: blocked, busy }}
      disabled={blocked}
      className={cn(
        actionVariants({ variant, size, disabled: blocked }),
        "focus:outline-2 focus:outline-offset-2 focus:outline-focus",
        className,
      )}
    >
      {(state) => (
        <>
          {busy && <ActivityIndicator size="small" color={colors.textMuted} />}
          <Text
            className={cn(actionLabelVariants({ variant, disabled: blocked }), "min-w-0 shrink text-center")}
          >
            {label}
          </Text>
          {typeof children === "function" ? children(state) : children}
        </>
      )}
    </Pressable>
  );
}

export function Label({
  variant,
  tone,
  className,
  ...props
}: TextProps & TextVariantProps & { className?: string }) {
  return (
    <Text
      {...props}
      className={cn(textVariants({ variant, tone }), className)}
    />
  );
}

type FieldProps = TextInputProps & { invalid?: boolean; className?: string };
export const Field = forwardRef<TextInput, FieldProps>(function Field(
  { invalid = false, className, placeholderTextColor, ...props },
  ref,
) {
  const { colors } = useAppTheme();
  return (
    <TextInput
      {...props}
      ref={ref}
      placeholderTextColor={placeholderTextColor ?? colors.textMuted}
      className={cn(inputVariants({ invalid }), "focus:outline-2 focus:outline-offset-2 focus:outline-focus", className)}
    />
  );
});

export function Surface({
  variant,
  className,
  ...props
}: ViewProps & SurfaceVariantProps & { className?: string }) {
  return (
    <View {...props} className={cn(surfaceVariants({ variant }), className)} />
  );
}

export function Notice({
  tone = "info",
  className,
  ...props
}: ViewProps & { tone?: "info" | "error"; className?: string }) {
  return (
    <View
      {...props}
      accessibilityRole={tone === "error" ? "alert" : undefined}
      accessibilityLiveRegion={tone === "error" ? "assertive" : "polite"}
      className={cn(noticeVariants({ tone }), className)}
    />
  );
}
