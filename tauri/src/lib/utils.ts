// HACK: temp workaround to import the same component in web-app
// Using relative import instead of @/ alias for cross-project compatibility
import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
/**
 * Prevents macOS from App-Napping the WKWebView WebContent XPC process.
 *
 * macOS App Nap exempts processes that are "playing audible audio", but it
 * must be non-zero output at the CoreAudio level. A zero-gain signal is
 * detected as silence and ignored. We use a 20 kHz tone (above human hearing)
 * at very low gain (−40 dB / 0.01) so CoreAudio sees real audio output while
 * nothing is audible.
 *
 * WebKit source (WebPageProxy.cpp) confirms this: when IsAudible is true,
 * it calls takeAudibleActivity() which holds a foreground assertion on the
 * WebContent process, preventing suspension.
 *
 * Source (pain to find in cs.github.com as its not indexed):
 * https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/WebPageProxy.cpp#L3440-L3443
 *
 * WKWebView probably (no freaking clue anymore) requires a user gesture before an AudioContext can run, so we
 * wait for the first interaction event before starting, any interaction like mouse move or keydown will do.
 *
 * Tried to set the Tauri's flag `backgroundThrottling` to `disabled` but it didn't work.
 */
export function disableWebViewAppNap(): void {
  const ctx = new AudioContext();
  const oscillator = ctx.createOscillator();
  oscillator.frequency.value = 20000;
  const gain = ctx.createGain();
  gain.gain.value = 0.01;
  oscillator.connect(gain);
  gain.connect(ctx.destination);

  let started = false;

  const start = () => {
    if (started) return;
    started = true;

    document.removeEventListener("click", start);
    document.removeEventListener("keydown", start);
    document.removeEventListener("mousemove", start);

    const resume = () => {
      if (ctx.state === "suspended") {
        ctx
          .resume()
          .then(() => {
            oscillator.start();
            console.log("starting background sound after suspension");
          })
          .catch((e) => {
            console.error("failed to resume AudioContext for App Nap prevention:", e);
          });
      } else {
        oscillator.start();
        console.log("starting background sound after app start");
      }
    };

    resume();

    ctx.addEventListener("statechange", () => {
      if (ctx.state === "suspended") {
        ctx.resume().catch((e) => {
          console.error("failed to resume AudioContext after statechange:", e);
        });
      }
    });
  };

  const registerStateChangeRecovery = () => {
    ctx.addEventListener("statechange", () => {
      if (ctx.state === "suspended") {
        ctx.resume().catch((e) => {
          console.error("failed to resume AudioContext after statechange:", e);
        });
      }
    });
  };

  if (ctx.state === "running") {
    console.warn("AudioContext was already running at construction time — this is unexpected in a WKWebView");
    started = true;
    oscillator.start();
    console.log("starting background sound");
    registerStateChangeRecovery();
  } else {
    document.addEventListener("click", start);
    document.addEventListener("keydown", start);
    document.addEventListener("mousemove", start);
  }
}
