import "@/services/sentry";
import "../../App.css";
import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import { useDisableNativeContextMenu, useSystemTheme } from "@/lib/hooks";
import { tauriUtils } from "../window-utils";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { Constants } from "@/constants";
import clsx from "clsx";
import createFetchClient from "openapi-fetch";
import type { paths } from "@/openapi";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <FeedbackWindow />
  </React.StrictMode>,
);

const EMOJI_RATINGS = [
  { emoji: "😡", label: "Awful", score: 1 },
  { emoji: "😕", label: "Poor", score: 2 },
  { emoji: "😐", label: "Okay", score: 3 },
  { emoji: "😊", label: "Good", score: 4 },
  { emoji: "🤩", label: "Great", score: 5 },
];

function FeedbackWindow() {
  useDisableNativeContextMenu();
  useSystemTheme();

  const [selectedScore, setSelectedScore] = useState<number | null>(null);
  const [feedbackText, setFeedbackText] = useState("");
  const [neverShowAgain, setNeverShowAgain] = useState(false);
  const [isSubmitting, setIsSubmitting] = useState(false);

  // Get params from URL
  const urlParams = new URLSearchParams(window.location.search);
  const teamId = urlParams.get("teamId") || "";
  const roomId = urlParams.get("roomId") || "";
  const participantId = urlParams.get("participantId") || "";

  const handleClose = async () => {
    if (neverShowAgain) {
      await tauriUtils.setFeedbackDisabled(true);
    }
    await getCurrentWebviewWindow().close();
  };

  const handleSubmit = async () => {
    if (selectedScore === null) return;

    setIsSubmitting(true);

    try {
      // Get stored token for authentication
      const token = await tauriUtils.getStoredToken();

      const client = createFetchClient<paths>({
        baseUrl: Constants.backendUrl,
        headers: token ? { Authorization: `Bearer ${token}` } : undefined,
      });

      const { error } = await client.POST("/api/auth/feedback", {
        body: {
          team_id: teamId,
          room_id: roomId,
          score: selectedScore,
          feedback: feedbackText || undefined,
          metadata: {
            participant_id: participantId,
            submitted_at: new Date().toISOString(),
          },
        },
      });

      if (error) {
        console.error("Failed to submit feedback:", error);
      }
    } catch (error) {
      console.error("Error submitting feedback:", error);
    } finally {
      setIsSubmitting(false);
      await handleClose();
    }
  };

  return (
    <div className="h-full min-h-full window-bg text-black dark:text-white flex flex-col">
      <div
        data-tauri-drag-region
        className="h-[32px] min-w-full w-full bg-transparent flex items-center justify-center"
      />

      {/* Content */}
      <div className="flex-1 flex flex-col px-8 pb-6 pt-2">
        {/* Title */}
        <h1 className="text-2xl font-semibold text-center mb-6 text-gray-900 dark:text-white">How was your call?</h1>

        {/* Emoji Rating */}
        <div className="flex justify-center gap-6 mb-8">
          {EMOJI_RATINGS.map(({ emoji, label, score }) => (
            <button
              key={score}
              onClick={() => setSelectedScore(score)}
              className={clsx(
                "flex flex-col items-center gap-1 transition-transform duration-150",
                selectedScore === score ? "scale-120" : "hover:scale-110",
              )}
            >
              <span
                className={clsx(
                  "text-4xl transition-all duration-150 select-none",
                  selectedScore === score ? "drop-shadow-lg" : (
                    "grayscale-30 opacity-80 hover:grayscale-0 hover:opacity-100"
                  ),
                )}
              >
                {emoji}
              </span>
              <span
                className={clsx(
                  "text-xs font-medium transition-colors",
                  selectedScore === score ? "text-gray-900 dark:text-white" : "text-gray-500 dark:text-gray-400",
                )}
              >
                {label}
              </span>
            </button>
          ))}
        </div>

        {/* Feedback Text Area */}
        <div className="mb-6">
          <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
            Did you face any issue?
          </label>
          <Textarea
            value={feedbackText}
            onChange={(e) => setFeedbackText(e.target.value)}
            placeholder="To err is human, to report bugs is divine"
            className="w-full min-h-[120px] resize-none bg-white dark:bg-[#2a2a2a] border border-gray-200 dark:border-gray-700 rounded-lg p-3 text-sm text-gray-900 dark:text-white placeholder:text-gray-400 dark:placeholder:text-gray-500 focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          />
        </div>

        <div className="flex flex-row items-center justify-between w-full">
          {/* Never Show Again Checkbox */}
          <label className="flex items-center gap-2 cursor-pointer select-none">
            <input
              type="checkbox"
              checked={neverShowAgain}
              onChange={(e) => setNeverShowAgain(e.target.checked)}
              className="w-4 h-4 rounded border-gray-300 dark:border-gray-600 text-blue-600 focus:ring-blue-500 focus:ring-2 cursor-pointer"
            />
            <span className="text-sm text-gray-600 dark:text-gray-400">Never show again</span>
          </label>

          {/* Action Buttons */}
          <div className="flex justify-end gap-3 mt-auto">
            <Button
              variant="ghost"
              onClick={handleClose}
              className="px-6 text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white"
            >
              Close
            </Button>
            <Button
              variant="default"
              onClick={handleSubmit}
              disabled={selectedScore === null || isSubmitting}
              isLoading={isSubmitting}
              className="px-6"
            >
              Submit
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
