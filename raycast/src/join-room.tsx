import { Action, ActionPanel, closeMainWindow, Icon, List, open, popToRoot } from "@raycast/api";
import { deepLink } from "./lib/hopp";
import type { Room, RoomsPresenceResponse } from "./lib/hopp";
import { useHoppApi } from "./lib/use-hopp-api";

/**
 * Join a Hopp room from Raycast. The extension lists the team's rooms from the
 * Hopp API and hands off to the desktop app through a hopp:// deep link, which
 * joins the room immediately — no need to click in the app.
 */
export default function Command() {
  const rooms = useHoppApi<Room[]>("/api/auth/rooms");
  const presence = useHoppApi<RoomsPresenceResponse>("/api/auth/rooms/presence");

  const error = rooms.error ?? presence.error;

  const refreshAction = (
    <Action
      title="Refresh"
      icon={Icon.ArrowClockwise}
      shortcut={{ modifiers: ["cmd"], key: "r" }}
      onAction={() => {
        rooms.revalidate();
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
    <List isLoading={rooms.isLoading || presence.isLoading} searchBarPlaceholder="Join a room…">
      {(rooms.data ?? []).map((room) => {
        const participants = presence.data?.rooms?.[room.id] ?? [];
        const participantText = participants.length === 0 ? "Empty" : `${participants.length} inside`;

        return (
          <List.Item
            key={room.id}
            icon={Icon.TwoPeople}
            title={room.name}
            accessories={[...(participants.length > 0 ? [{ icon: Icon.Person }] : []), { text: participantText }]}
            actions={
              <ActionPanel>
                <Action
                  title={`Join ${room.name}`}
                  icon={Icon.ArrowRightCircle}
                  onAction={() => openInHopp(deepLink.room(room.id))}
                />
                {refreshAction}
                <Action.CopyToClipboard title="Copy Room ID" content={room.id} />
              </ActionPanel>
            }
          />
        );
      })}
      {!rooms.isLoading && (rooms.data ?? []).length === 0 && (
        <List.EmptyView
          icon={Icon.TwoPeople}
          title="No rooms"
          description="Create a room in the Hopp app to join it from here."
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
