import { create as _create, StateCreator } from "zustand";
import { merge } from "lodash";
import { persist } from "zustand/middleware";
import { useEffect, useState } from "react";
import { createStateSlice, StateSlice } from "./state";

const storeResetFns = new Set<() => void>();

export const resetAllStores = () => {
  storeResetFns.forEach((resetFn) => {
    resetFn();
  });
};

/**
 * Reset all stores.
 * To work properly all the slices need to have a reset function on the first level
 * Example:
 *  {
 *      user: {
 *          ... // initial state
 *          resetSlice: () => void // callback to reset the slice
 *      },
 *      ...
 *  }
 * @see https://docs.pmnd.rs/zustand/guides/how-to-reset-state
 */
export const create = (<T extends object>() => {
  return (stateCreator: StateCreator<T>) => {
    const store = _create(stateCreator);
    const initialState = store.getState() as T;

    Object.keys(initialState).forEach((key) => {
      const entry = initialState[key as keyof T];

      if (typeof entry !== "object" || entry === null || !("resetSlice" in entry)) return;

      const resetFn = entry["resetSlice"];
      if (resetFn && typeof resetFn === "function") {
        storeResetFns.add(resetFn as unknown as () => void);
      }
    });
    return store;
  };
}) as typeof _create;

/**
 * Combined State, where all the values will be "flattened"
 * Maybe at some point we will need to separate
 * them instead of a flat store object
 */
export const useHoppStore = create<StateSlice>()(
  persist(
    (...a) => ({
      ...createStateSlice(...a),
    }),
    {
      name: "hopp-store",
      // https://github.com/pmndrs/zustand/blob/main/docs/integrations/persisting-store-data.md#merge
      merge: (persistedState, currentState) => merge(currentState, persistedState),
    },
  ),
);

/**
 * Hook to check if the store has been hydrated.
 * Source:
 * https://docs.pmnd.rs/zustand/integrations/persisting-store-data#how-can-i-check-if-my-store-has-been-hydrated
 */
export const useHydration = () => {
  const [hydrated, setHydrated] = useState(false);

  useEffect(() => {
    const unsubFinishHydration = useHoppStore.persist.onFinishHydration(() => setHydrated(true));

    setHydrated(useHoppStore.persist.hasHydrated());

    return () => {
      unsubFinishHydration();
    };
  }, []);

  return hydrated;
};
