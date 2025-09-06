import React from "react";
import { IoClose } from "react-icons/io5";
import { VscChromeMinimize, VscChromeMaximize } from "react-icons/vsc";
import { cn } from "@/lib/utils";

interface WindowActionProps {
  onClick?: () => void;
  className?: string;
  children?: React.ReactNode;
}

interface WindowActionsType {
  Close: React.FC<WindowActionProps>;
  Minimize: React.FC<WindowActionProps>;
  Maximize: React.FC<WindowActionProps>;
  Empty: React.FC<WindowActionProps>;
}

const WindowActionBase: React.FC<WindowActionProps & { children: React.ReactNode }> = ({
  onClick,
  className,
  children,
}) => {
  return (
    <div
      className={cn(
        "size-[14px] rounded-full bg-white/50 text-gray-600 flex items-center justify-center cursor-pointer hover:bg-white/70 transition-colors",
        className,
      )}
      onClick={onClick}
    >
      {children}
    </div>
  );
};

const WindowActionsClose: React.FC<WindowActionProps> = ({ onClick, className }) => {
  return (
    <WindowActionBase onClick={onClick} className={className}>
      <IoClose />
    </WindowActionBase>
  );
};

const WindowActionsMinimize: React.FC<WindowActionProps> = ({ onClick, className }) => {
  return (
    <WindowActionBase onClick={onClick} className={className}>
      <VscChromeMinimize />
    </WindowActionBase>
  );
};

const WindowActionsMaximize: React.FC<WindowActionProps> = ({ onClick, className }) => {
  return (
    <WindowActionBase onClick={onClick} className={className}>
      <VscChromeMaximize />
    </WindowActionBase>
  );
};

const WindowActionsEmpty: React.FC<WindowActionProps> = ({ onClick, className, children }) => {
  return (
    <WindowActionBase onClick={onClick} className={className}>
      {children}
    </WindowActionBase>
  );
};

export const WindowActions: WindowActionsType = {
  Close: WindowActionsClose,
  Minimize: WindowActionsMinimize,
  Maximize: WindowActionsMaximize,
  Empty: WindowActionsEmpty,
};
