// HACK: temp workaround to import the same component in web-app
// Using relative import instead of @/ alias for cross-project compatibility
import { TPMouseMove } from "../payloads";
import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export const getRelativePosition = (el: HTMLElement, event: MouseEvent) => {
  const rect = el.getBoundingClientRect();

  const relativeX = (event.clientX - rect.left) / rect.width; // X relative to the video
  const relativeY = (event.clientY - rect.top) / rect.height; // Y relative to the video

  return { relativeX, relativeY };
};

export const getAbsolutePosition = (el: HTMLElement, mousePos: TPMouseMove) => {
  const rect = el.getBoundingClientRect();

  const absoluteX = mousePos.payload.x * rect.width;
  const absoluteY = mousePos.payload.y * rect.height;

  return { absoluteX, absoluteY };
};

export const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export async function getMicrophones(): Promise<MediaDeviceInfo[]> {
  const devices = await navigator.mediaDevices.enumerateDevices();
  return devices.filter((d) => d.kind === "audioinput");
}
