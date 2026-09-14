import { useCallback, useEffect, useRef, useState } from "react";
import {
  PreferencePersistenceController,
  type PreferencePersistenceState,
} from "./preference-persistence";

export function usePreferencePersistence<T>(
  createController: () => PreferencePersistenceController<T>,
  initialValue: T,
) {
  const controllerRef = useRef<PreferencePersistenceController<T> | null>(null);
  const [state, setState] = useState<PreferencePersistenceState<T>>({
    value: initialValue,
    status: "loading",
  });

  useEffect(() => {
    const controller = createController();
    controllerRef.current = controller;
    const unsubscribe = controller.subscribe(setState);
    void controller.load();
    return () => {
      if (controllerRef.current === controller) controllerRef.current = null;
      unsubscribe();
      controller.dispose();
    };
  }, [createController]);

  const update = useCallback((updater: (value: T) => T) => {
    const controller = controllerRef.current;
    if (controller) controller.setValue(updater(controller.state.value));
  }, []);

  const retry = useCallback(() => {
    const controller = controllerRef.current;
    if (controller) void controller.retry();
  }, []);

  return { retry, state, update };
}
