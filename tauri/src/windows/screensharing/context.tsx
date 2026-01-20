import React, { createContext, useContext, useState, useCallback, ReactNode } from "react";
import { TDrawingMode } from "@/payloads";

type SharingContextType = {
  isSharingMouse: boolean;
  isSharingKeyEvents: boolean;
  drawingMode: TDrawingMode;
  videoToken: string | null;
  setIsSharingMouse: (value: boolean) => void;
  setIsSharingKeyEvents: (value: boolean) => void;
  setDrawingMode: (value: TDrawingMode) => void;
  setVideoToken: (value: string) => void;
  parentKeyTrap?: HTMLDivElement;
  setParentKeyTrap: (value: HTMLDivElement) => void;
  streamDimensions: { width: number; height: number } | null;
  setStreamDimensions: (value: { width: number; height: number } | null) => void;
  rightClickToClear: boolean;
  setRightClickToClear: (value: boolean) => void;
  // Signal to trigger clearing all drawings - components watch for changes to this value
  clearDrawingsSignal: number;
  triggerClearDrawings: () => void;
  // Flag to block resizeWindow calls during programmatic resizing
  isProgrammaticResize: boolean;
  setIsProgrammaticResize: (value: boolean) => void;
};

const SharingContext = createContext<SharingContextType | undefined>(undefined);

export const useSharingContext = (): SharingContextType => {
  const context = useContext(SharingContext);
  if (!context) {
    throw new Error("useSharingContext must be used within a SharingProvider");
  }
  return context;
};

type SharingProviderProps = {
  children: ReactNode;
};

export const SharingProvider: React.FC<SharingProviderProps> = ({ children }) => {
  const [isSharingMouse, setIsSharingMouse] = useState<boolean>(true);
  const [isSharingKeyEvents, setIsSharingKeyEvents] = useState<boolean>(true);
  const [drawingMode, setDrawingMode] = useState<TDrawingMode>({ type: "Disabled" });
  const [parentKeyTrap, setParentKeyTrap] = useState<HTMLDivElement | undefined>(undefined);
  const [videoToken, setVideoToken] = useState<string | null>(null);
  const [streamDimensions, setStreamDimensions] = useState<{ width: number; height: number } | null>(null);
  const [rightClickToClear, setRightClickToClear] = useState<boolean>(false);
  const [clearDrawingsSignal, setClearDrawingsSignal] = useState<number>(0);
  const [isProgrammaticResize, setIsProgrammaticResize] = useState<boolean>(false);

  const triggerClearDrawings = useCallback(() => {
    setClearDrawingsSignal((prev) => prev + 1);
  }, []);

  return (
    <SharingContext.Provider
      value={{
        isSharingMouse,
        isSharingKeyEvents,
        drawingMode,
        setIsSharingMouse,
        setIsSharingKeyEvents,
        setDrawingMode,
        parentKeyTrap,
        setParentKeyTrap,
        videoToken,
        setVideoToken,
        streamDimensions,
        setStreamDimensions,
        rightClickToClear,
        setRightClickToClear,
        clearDrawingsSignal,
        triggerClearDrawings,
        isProgrammaticResize,
        setIsProgrammaticResize,
      }}
    >
      {children}
    </SharingContext.Provider>
  );
};
