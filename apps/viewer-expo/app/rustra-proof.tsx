import { useEffect, useState } from "react";
import { Text, View } from "react-native";
import { initializeRustra } from "../src/rustra";
import { GENERATED_CONTRACT_HASH } from "../generated/contract";

/**
 * Device-proof route for the Rustra bridge (E9).
 * Invokes the generated JSI bridge end to end and renders the result.
 * Not linked from app navigation; reached via deep link only
 * (`leftcar://rustra-proof`) — see CONTRIBUTING.md "Rustra 코드젠".
 */
export default function RustraProofScreen() {
  const [result, setResult] = useState<string>("pending");

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const out = await initializeRustra().commands.addNumbers({ a: 20, b: 22 });
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
    <View style={{ flex: 1, alignItems: "center", justifyContent: "center" }}>
      <Text testID="rustra-proof-result">{result}</Text>
      <Text testID="rustra-proof-hash">{GENERATED_CONTRACT_HASH.slice(0, 16)}</Text>
    </View>
  );
}
