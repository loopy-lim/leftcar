import { View } from "nsh";
import { Sheet } from "nsh/base";
import { language, setLanguage, t } from "../i18n";
import { Action } from "../ui/Action";
import { Label } from "../ui/Label";

export interface SettingsSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function SettingsSheet(props: SettingsSheetProps) {
  const title = () => t().viewer.settingsTitle;
  return (
    <Sheet
      open={props.open}
      onOpenChange={(event) => props.onOpenChange(event.open)}
      closeOnSwipe
      accessibilityLabel={title()}
      popupClass="mx-auto w-full max-w-3xl rounded-t-(--radius-xl) border border-outline bg-canvas p-4"
      backdropClass="bg-scrim"
    >
      <View class="gap-5 pb-4">
        <View class="flex-row items-center justify-between gap-3">
          <Label variant="title" class="flex-1">
            {title()}
          </Label>
          <Action
            variant="ghost"
            size="icon"
            label="×"
            accessibilityLabel={`${title()}: ${t().common.close}`}
            onPress={() => props.onOpenChange(false)}
            testID="settings-close"
          />
        </View>
        <LanguageSection />
      </View>
    </Sheet>
  );
}

function LanguageSection() {
  return (
    <View class="gap-2">
      <Label variant="title">{t().common.languageTitle}</Label>
      <View class="flex-row gap-2">
        <Action
          variant={language() === "ko" ? "primary" : "secondary"}
          selected={language() === "ko"}
          label="한국어"
          onPress={() => setLanguage("ko")}
          testID="language-ko"
        />
        <Action
          variant={language() === "en" ? "primary" : "secondary"}
          selected={language() === "en"}
          label="English"
          onPress={() => setLanguage("en")}
          testID="language-en"
        />
      </View>
    </View>
  );
}
