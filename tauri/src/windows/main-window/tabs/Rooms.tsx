import { BACKEND_URLS } from "@/constants";
import { components } from "@/openapi";
import { sounds } from "@/constants/sounds";
import { useAPI } from "@/services/query";
import { Input } from "@/components/ui/input";
import Fuse from "fuse.js";
import { Plus,MoreHorizontal } from "lucide-react"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";import { SelectPortal } from "@radix-ui/react-select";
import { RoomButton } from "@/components/ui/room-button";
import useStore, { ParticipantRole } from "@/store/store";
import { useCallback, useMemo } from "react";
import toast from "react-hot-toast";
import { writeText, readText } from "@tauri-apps/plugin-clipboard-manager";
import { useParticipants } from "@livekit/components-react";
import { HoppAvatar } from "@/components/ui/hopp-avatar";
import { Button } from "@/components/ui/button";
import { HiMiniLink } from "react-icons/hi2";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { HiMagnifyingGlass } from "react-icons/hi2";
import { useState } from "react";

interface RoomProps {
  id: components["schemas"]["Room"]["id"];
  name: components["schemas"]["Room"]["name"];
}

interface RoomsProps {
  rooms: components["schemas"]["Room"][];
}

const fuseSearch = ({rooms}: RoomsProps, searchQuery: string) => {
  const fuse = new Fuse(rooms, {
    keys: ["name"],
    threshold: 0.3,
    shouldSort: true,
  });
  return fuse.search(searchQuery).map((result) => result.item);
};



export const Rooms = () => {
  const {authToken, callTokens, setCallTokens} = useStore()
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedRoom, setSelectedRoom] = useState<RoomProps>({id: "", name: ""})

  const { useQuery } = useAPI();

  // Get current user's rooms
const { error: roomsError, data: rooms } = useQuery(
    "get", 
    "/api/auth/rooms", 
    undefined,
    {
      enabled: !!authToken,
      refetchIntervalInBackground: true,
      retry: true,
      queryHash: `rooms-${authToken}`,
    }
  );


  const { useMutation } = useAPI();
  const { mutateAsync: getRoomTokens, error } = useMutation("get", "/api/auth/room/{id}", undefined);
  const handleJoinRoom = useCallback(async (room: RoomProps) => {
    try {
      const tokens = await getRoomTokens({
        params: {
          path: {
            id: room.id
          }
        }
      });
      if (!tokens) {
        toast.error("Error joining room");
        return;
      }
      sounds.callAccepted.play();
      setSelectedRoom(room)
      setCallTokens({
        ...tokens,
        isRoomCall: true,
        timeStarted: new Date(),
        hasAudioEnabled: true,
        role: ParticipantRole.NONE,
        isRemoteControlEnabled: true,
        cameraTrackId: null,
      });
    } catch (error) {
      toast.error("Error joining room");
    }
  }, [getRoomTokens]);

  return (
    <div className="flex flex-col items-start gap-1.5 p-2">
  <div className="flex flex-col gap-2 w-full">
    <div className="flex items-center gap-2 w-full">
      <div className="relative flex-1">
        <HiMagnifyingGlass className="absolute left-2 top-1/2 transform -translate-y-1/2 text-gray-500 size-4" />
        <Input
          type="text"
          placeholder="Search rooms..."
          className="pl-8 w-full focus-visible:ring-opacity-20 focus-visible:ring-2 focus-visible:ring-blue-300"
        />
      </div>
      <Button variant="secondary" size="icon" className="size-8">
        <Plus />
      </Button>
    </div>
    <div className="grid grid-cols-2 gap-2 w-full">
      <RoomButton
        size="unsized"
        className="flex-1 min-w-0 text-slate-600"
        cornerIcon={
          <DropdownMenu>
          <DropdownMenuTrigger className="hover:outline-solid hover:outline-1 hover:outline-slate-300 focus:ring-0 focus-visible:ring-0 hover:bg-slate-200 size-4 rounded-xs p-0 border-0 shadow-none hover:shadow-xs">
            {/* Add your trigger icon here, e.g., three dots */}
            <MoreHorizontal className="size-3" />
          </DropdownMenuTrigger>
          <DropdownMenuContent>
            <DropdownMenuItem onClick={() => console.log("Copy room link clicked")}>
              Copy room link
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() =>  console.log("Favorite room clicked")}>
              Favorite room
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() =>  console.log("Subscribe clicked")}>
              Subscribe
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() =>  console.log("More clicked")}>
              More
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        }
      >
      </RoomButton>
    </div>
  </div>
</div>
  );
};

const SelectedRoom = (room: RoomProps) => {
  const { useMutation } = useAPI();
  const participants = useParticipants();
  const { teammates, user } = useStore();

  const { mutateAsync: getRoomAnonymous, error: errorAnonymous } = useMutation(
    "get",
    "/api/auth/room/anonymous",
    undefined,
  );
  console.log("Here?",room.id)
  const handleInviteAnonymousUser = useCallback(async () => {
    const redirectURL = await getRoomAnonymous({});
    if (!redirectURL || !redirectURL.redirect_url) {
      toast.error("Error generating link");
      return;
    }
    const link = `${BACKEND_URLS.BASE}${redirectURL.redirect_url}`;
    await writeText(link);
    toast.success("Link copied to clipboard");
  }, [getRoomAnonymous]);

  // Parse participant identities and match with teammates
  const participantList = useMemo(() => {
    return participants
      .filter((participant) => !participant.identity.includes("video") && !participant.identity.includes("camera"))
      .map((participant) => {
        // Parse identity: format is "room:roomname:participantId:tracktype"
        // Extract participantId by splitting on ":" and taking the second-to-last part
        const identityParts = participant.identity.split(":");
        let participantId: string;

        if (identityParts.length >= 4) {
          // Format: "room:roomname:participantId:tracktype"
          participantId = identityParts[2] || participant.identity;
        } else {
          participantId = participant.identity;
        }

        // Handle anonymous participants
        if (participantId === "anonymous" || !participantId) {
          return {
            id: participant.identity,
            participantId: "anonymous",
            user: null,
            isLocal: participant.isLocal,
          };
        }

        // Find user in teammates or current user
        let foundUser = null;
        if (user && user.id === participantId) {
          foundUser = user;
        } else if (teammates) {
          foundUser = teammates.find((teammate) => teammate.id === participantId);
        }

        return {
          id: participant.identity,
          participantId,
          user: foundUser,
          isLocal: participant.isLocal,
        };
      });
  }, [participants, teammates, user]);

  return (
    <div className="flex flex-col w-full">
      <div className="flex flex-row gap-2 justify-between items-center mb-4">
        <div>
          <h3 className="small">Watercooler 🚰</h3>
          <span className="text-xs font-medium text-slate-600 mb-2">Participants ({participantList.length})</span>
        </div>
        <div className="flex flex-row gap-2">
          <Button
            variant="outline"
            size="icon-sm"
            onClick={() => {
              handleInviteAnonymousUser();
            }}
          >
            <TooltipProvider delayDuration={100}>
              <Tooltip>
                <TooltipTrigger>
                  <HiMiniLink className="size-3.5" />
                </TooltipTrigger>
                <TooltipContent side="left" sideOffset={10} className="flex flex-col items-center gap-0">
                  <span>Invite anonymous user</span>
                  <span className="text-xs text-slate-400">expires in 10 mins</span>
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          </Button>
        </div>
      </div>
      <div className="flex flex-col gap-2">
        <div className="flex flex-col gap-3">
          {participantList.map((participant) => (
            <div key={participant.id} className="flex items-center gap-3">
              {participant.user ?
                <>
                  <HoppAvatar
                    src={participant.user.avatar_url || undefined}
                    firstName={participant.user.first_name}
                    lastName={participant.user.last_name}
                    status="online"
                  />
                  <div className="flex flex-col">
                    <span className="text-sm font-medium">
                      {participant.user.first_name} {participant.user.last_name}
                      {participant.isLocal && " (You)"}
                    </span>
                  </div>
                </>
              : <>
                  <div className="w-8 h-8 rounded-full bg-slate-200 flex items-center justify-center">
                    <span className="text-xs font-medium text-slate-600">?</span>
                  </div>
                  <div className="flex flex-col">
                    <span className="text-sm font-medium text-slate-600">
                      Anonymous user
                      {participant.isLocal && " (You)"}
                    </span>
                    <span className="text-xs text-slate-500">Unknown participant</span>
                  </div>
                </>
              }
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};
