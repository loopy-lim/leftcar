import { useEffect, useState } from "react";
import { router } from "expo-router";
import type { TranslationSchema } from "../i18n";
import type { KmNativeStatus } from "../km-session";
import { kmNative, subscribeKmStatus } from "../km.native";
import { shouldShowKmEntry } from "../km-entry";
import { Action } from "../ui/primitives";

/** Native sharing can outlive the screen's default/video control connection. */
export function InputSharingEntry({ connected, t }: { connected: boolean; t: TranslationSchema }) {
  const [state, setState] = useState<KmNativeStatus["state"] | null>(null);
  useEffect(() => {
    let mounted = true;
    const unsubscribe = subscribeKmStatus(status => { if (mounted) setState(status.state); });
    void kmNative?.getStatus().then(status => { if (mounted) setState(status.state); }).catch(() => undefined);
    return () => { mounted = false; unsubscribe(); };
  }, []);
  if (!shouldShowKmEntry(connected, state)) return null;
  return <Action variant="secondary" onPress={() => router.push("/input-sharing")} label={t.km.title} />;
}
