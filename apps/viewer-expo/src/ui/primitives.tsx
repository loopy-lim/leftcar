import {
  createContext,
  forwardRef,
  useContext,
  useMemo,
  type ComponentProps,
  type ReactNode,
} from "react";
import {
  ActivityIndicator,
  Pressable,
  Text,
  TextInput,
  View,
  useWindowDimensions,
  type PressableProps,
  type StyleProp,
  type TextInputProps,
  type TextProps,
  type TextStyle,
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
  typography,
  TEXT_VARIANT_TYPE_ROLES,
  type ActionVariantProps,
  type SurfaceVariantProps,
  type TextVariantProps,
} from "@leftcar/ui-tokens";
import { useAppTheme } from "../theme";
import { panelDensityScale } from "../panel-density";
import { SafeAreaView } from "react-native-safe-area-context";
import { withUniwind } from "uniwind";

// The Fabric safe-area component is not a core React Native style target.
const StyledSafeArea = withUniwind(SafeAreaView);

/**
 * 패널 밀도 배율(XR 리뷰 §B). 72ff990에서 유틸리티 클래스 전환으로 스타일
 * 객체 변환(applyPanelDensity) 호출이 사라진 것을 이 계층에서 복원한다 —
 * SafeArea가 화면 폭으로 배율을 정해 내려보내고, 텍스트 프리미티브가
 * 변형별 토큰 크기에 배율을 곡한다. 네이티브 HUD(StreamPanelDensity.kt)와
 * 같은 곡선(panel-density.ts)을 쓴다.
 */
const PanelDensityContext = createContext(1);

export function PanelDensityProvider({ children }: { children: ReactNode }) {
  const { width } = useWindowDimensions();
  const scale = panelDensityScale(width);
  return (
    <PanelDensityContext.Provider value={scale}>
      {children}
    </PanelDensityContext.Provider>
  );
}

const BASE_TEXT_VARIANT = "body" as const;

/** 변형의 토큰 크기에 밀도 배율을 곡한 인라인 스타일. 배율 1이면 원본이다. */
function useScaledTextStyle(
  variant: TextVariantProps["variant"],
  style?: StyleProp<TextStyle>,
): StyleProp<TextStyle> {
  const scale = useContext(PanelDensityContext);
  return useMemo(() => {
    if (scale === 1) return style;
    const role = TEXT_VARIANT_TYPE_ROLES[variant ?? BASE_TEXT_VARIANT];
    return [
      style,
      {
        fontSize: Math.round(typography.fontSize[role] * scale),
        lineHeight: Math.round(typography.lineHeight[role] * scale),
      },
    ];
  }, [scale, style, variant]);
}
export function SafeArea(props: ComponentProps<typeof StyledSafeArea>) {
  // 화면 진입점에서 패널 밀도를 한 번 정의한다 — 4개 화면(index/catalog/
  // host/pairing)이 전부 SafeArea를 쓰므로 여기만 고치면 XR 대형 패널의
  // 폰 정체감(§B)이 함께 풀린다.
  return (
    <PanelDensityProvider>
      <StyledSafeArea {...props} />
    </PanelDensityProvider>
  );
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
          <ActionLabelText
            variant={variant}
            blocked={blocked}
            className="min-w-0 shrink text-center"
          >
            {label}
          </ActionLabelText>
          {typeof children === "function" ? children(state) : children}
        </>
      )}
    </Pressable>
  );
}

/** 액션 라벨은 항상 body 크기다(actionLabelVariants 기본). 밀도 배율을 같이 적용한다. */
function ActionLabelText({
  variant,
  blocked,
  children,
  className,
}: {
  variant: ActionVariantProps["variant"];
  blocked: boolean;
  children: ReactNode;
  className?: string;
}) {
  const scaledStyle = useScaledTextStyle(BASE_TEXT_VARIANT, undefined);
  return (
    <Text
      style={scaledStyle}
      className={cn(
        actionLabelVariants({ variant, disabled: blocked }),
        className,
      )}
    >
      {children}
    </Text>
  );
}

export function Label({
  variant,
  tone,
  className,
  style,
  ...props
}: TextProps & TextVariantProps & { className?: string }) {
  // 인라인 스타일이 변형 클래스 뒤에 와야 밀도 배율이 이긴다.
  const scaledStyle = useScaledTextStyle(variant, style);
  return (
    <Text
      {...props}
      style={scaledStyle}
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
