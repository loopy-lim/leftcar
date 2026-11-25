import { colors, radii, typography, type ThemeTokens } from "./tokens";
import { TEXT_VARIANT_TYPE_ROLES, colorRoles } from "./theme-css";

const shadcnRoles = {
  background: "bgCanvas", foreground: "textPrimary",
  card: "bgSurface", "card-foreground": "textPrimary",
  popover: "bgSurface", "popover-foreground": "textPrimary",
  primary: "btnPrimaryBg", "primary-foreground": "btnPrimaryText",
  "secondary-foreground": "textPrimary", "muted-foreground": "textMuted",
  accent: "bgHover", "accent-foreground": "textPrimary",
  destructive: "statusDanger",
  border: "borderCard", input: "borderCard", ring: "borderFocus",
} satisfies Record<string, keyof ThemeTokens>;

function lengths(): Record<string, string> {
  const text = Object.entries(TEXT_VARIANT_TYPE_ROLES).flatMap(([role, size]) => [
    [`text-${role}`, `${typography.fontSize[size]}px`],
    [`leading-${role}`, `${typography.lineHeight[size]}px`],
  ]);
  const corners = (["sm", "md", "lg", "xl"] as const).map((radius) => [`radius-${radius}`, `${radii[radius]}px`]);
  return Object.fromEntries([...text, ...corners]);
}

function appearance(mode: "light" | "dark"): Record<string, string> {
  const palette = colors[mode];
  const vars = Object.fromEntries(
    Object.entries({ ...colorRoles, ...shadcnRoles }).map(([role, key]) => [role, palette[key]]),
  );
  return { ...vars, ...lengths() };
}

export function renderNshTheme(): string {
  return `${JSON.stringify({ cssVars: { light: appearance("light"), dark: appearance("dark") }, radius: `${radii.md}px` }, null, 2)}\n`;
}
