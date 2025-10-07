import React, { createContext, useContext, useState, ReactNode } from "react";

type SharingContextType = {
  isSharingMouse: boolean;
  isSharingKeyEvents: boolean;
  videoToken: string | null;
  setIsSharingMouse: (value: boolean) => void;
  setIsSharingKeyEvents: (value: boolean) => void;
  setVideoToken: (value: string) => void;
  parentKeyTrap?: HTMLDivElement;
  setParentKeyTrap: (value: HTMLDivElement) => void;
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
  const [parentKeyTrap, setParentKeyTrap] = useState<HTMLDivElement | undefined>(undefined);
  const [videoToken, setVideoToken] = useState<string | null>(null);

  return (
    <SharingContext.Provider
      value={{
        isSharingMouse,
        isSharingKeyEvents,
        setIsSharingMouse,
        setIsSharingKeyEvents,
        parentKeyTrap,
        setParentKeyTrap,
        videoToken,
        setVideoToken,
      }}
    >
      {children}
    </SharingContext.Provider>
  );
};
