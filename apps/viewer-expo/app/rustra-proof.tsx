import { useEffect, useState } from "react";
import { router } from "expo-router";
import { Action, Badge, Card, Label, SafeArea } from "../src/ui/primitives";
import { useAppLanguage } from "../src/i18n";
import { initializeRustra } from "../src/rustra";
import { GENERATED_CONTRACT_HASH } from "../generated/contract";

/**
 * Device-proof route for the Rustra/UniKit bridge (E9).
 * Invokes the generated JSI bridge end to end and renders the result.
 * Not linked from app navigation; reached via deep link only
 * (`leftcar://rustra-proof`) — see CONTRIBUTING.md "Rustra 코드젠".
 */
export default function RustraProofScreen() {
  const { t } = useAppLanguage();
  const [result, setResult] = useState<string>("pending");

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const out = await initializeRustra().commands.addNumbers({
          a: 20,
          b: 22,
        });
        if (!cancelled) setResult(`value=${out.value}`);
      } catch (error) {
        if (!cancelled) setResult(`error=${String(error)}`);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <SafeArea
      className="flex-1 items-center justify-center gap-3 bg-canvas px-4"
      edges={["top", "left", "right", "bottom"]}
    >
      <Card className="w-full max-w-sm items-center gap-3 p-4">
        <Badge variant={result.startsWith("value") ? "default" : "secondary"}>
          UniKit Bridge
        </Badge>
        <Label variant="code" testID="rustra-proof-result">
          {result}
        </Label>
        <Label variant="code" testID="rustra-proof-hash">
          {GENERATED_CONTRACT_HASH.slice(0, 16)}
        </Label>
        <Action
          variant="secondary"
          label={t.common.back}
          onPress={() => router.replace("/")}
        />
      </Card>
    </SafeArea>
  );
}
