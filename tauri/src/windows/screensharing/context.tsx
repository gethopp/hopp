import React, { createContext, useContext, useState, ReactNode } from "react";
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
      }}
    >
      {children}
    </SharingContext.Provider>
  );
};
