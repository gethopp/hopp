import { Action, ActionPanel, closeMainWindow, Icon, List, open, popToRoot } from "@raycast/api";
import { deepLink, presenceText, teammateName } from "./lib/hopp";
import type { CallsPresenceResponse, Teammate } from "./lib/hopp";
import { useHoppApi } from "./lib/use-hopp-api";

/**
 * Call a teammate from Raycast. The extension lists the user's teammates from
 * the Hopp API and hands off to the desktop app through hopp:// deep links:
 * teammates in an ongoing call get "Join Call", everyone else gets "Call"
 * which rings them in the app.
 */
export default function Command() {
  const teammates = useHoppApi<Teammate[]>("/api/auth/teammates");
  const presence = useHoppApi<CallsPresenceResponse>("/api/auth/calls/presence");

  const error = teammates.error ?? presence.error;

  const refreshAction = (
    <Action
      title="Refresh"
      icon={Icon.ArrowClockwise}
      shortcut={{ modifiers: ["cmd"], key: "r" }}
      onAction={() => {
        teammates.revalidate();
        presence.revalidate();
      }}
    />
  );

  if (error) {
    return (
      <List>
        <List.EmptyView
          icon={Icon.ExclamationMark}
          title="Couldn't reach Hopp"
          description={error.message}
          actions={<ActionPanel>{refreshAction}</ActionPanel>}
        />
      </List>
    );
  }

  return (
    <List isLoading={teammates.isLoading || presence.isLoading} searchBarPlaceholder="Call a teammate…">
      {(teammates.data ?? []).map((teammate) => {
        const userPresence = presence.data?.presence?.[teammate.id];
        const inCall = !!userPresence;
        const title = teammateName(teammate);
        const status = presenceText(teammate, userPresence);

        return (
          <List.Item
            key={teammate.id}
            icon={teammate.avatar_url ? { source: teammate.avatar_url } : Icon.Person}
            title={title}
            accessories={[...(inCall ? [{ icon: Icon.Phone }] : []), { text: status }]}
            actions={
              <ActionPanel>
                {inCall ?
                  <Action
                    title={`Join ${title}'s Call`}
                    icon={Icon.Phone}
                    onAction={() => openInHopp(deepLink.joinCall(teammate.id))}
                  />
                : <Action
                    title={`Call ${title}`}
                    icon={Icon.Phone}
                    onAction={() => openInHopp(deepLink.call(teammate.id))}
                  />
                }
                {inCall && (
                  <Action
                    title={`Call ${title}`}
                    icon={Icon.PhoneRinging}
                    shortcut={{ modifiers: ["cmd", "shift"], key: "c" }}
                    onAction={() => openInHopp(deepLink.call(teammate.id))}
                  />
                )}
                {refreshAction}
                <Action.CopyToClipboard title="Copy User ID" content={teammate.id} />
              </ActionPanel>
            }
          />
        );
      })}
      {!teammates.isLoading && (teammates.data ?? []).length === 0 && (
        <List.EmptyView
          icon={Icon.Person}
          title="No teammates"
          description="Invite teammates to your Hopp team to call them from here."
          actions={<ActionPanel>{refreshAction}</ActionPanel>}
        />
      )}
    </List>
  );
}

async function openInHopp(url: string): Promise<void> {
  await closeMainWindow();
  await popToRoot();
  await open(url);
}
