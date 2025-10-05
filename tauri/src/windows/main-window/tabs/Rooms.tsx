import { BACKEND_URLS } from "@/constants";
import { components } from "@/openapi";
import { sounds } from "@/constants/sounds";
import { useAPI } from "@/services/query";
import { Input } from "@/components/ui/input";
import Fuse from "fuse.js";
import { FormEvent } from "react";
import { Plus, MoreHorizontal } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { RoomButton } from "@/components/ui/room-button";
import useStore, { ParticipantRole } from "@/store/store";
import { useCallback, useEffect, useMemo } from "react";
import toast from "react-hot-toast";
import { writeText, readText } from "@tauri-apps/plugin-clipboard-manager";
import { useParticipants, useRoomContext } from "@livekit/components-react";
import { RoomEvent } from "livekit-client";
import { HoppAvatar } from "@/components/ui/hopp-avatar";
import { Button } from "@/components/ui/button";
import { HiMiniLink } from "react-icons/hi2";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { HiMagnifyingGlass, HiOutlinePencil, HiOutlineTrash } from "react-icons/hi2";
import { useState } from "react";
import doorImage from "@/assets/door.png";

type Room = components["schemas"]["Room"];

const fuseSearch = (rooms: Room[], searchQuery: string) => {
  const fuse = new Fuse(rooms, {
    keys: ["name"],
    threshold: 0.3,
    shouldSort: true,
  });
  return fuse.search(searchQuery).map((result) => result.item);
};

export const Rooms = () => {
  const { authToken, callTokens, setCallTokens } = useStore();
  const [searchQuery, setSearchQuery] = useState("");
  const [filteredRooms, setFilteredRooms] = useState<Room[]>([]);
  const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false);
  const [isUpdateDialogOpen, setIsUpdateDialogOpen] = useState(false);
  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false);
  const [selectedRoom, setSelectedRoom] = useState<Room | null>(null);

  const { useQuery } = useAPI();

  // Get current user's rooms
  const {
    error: roomsError,
    data: rooms,
    refetch,
  } = useQuery("get", "/api/auth/rooms", undefined, {
    enabled: !!authToken,
    refetchInterval: 30_000,
    retry: true,
    queryHash: `rooms-${authToken}`,
    // select: (data) => {
    //   setRooms(data);
    // },
  });

  const { useMutation } = useAPI();
  const { mutateAsync: getRoomTokens, error } = useMutation("get", "/api/auth/room/{id}", undefined);

  const { mutateAsync: createRoom } = useMutation("post", "/api/auth/room", undefined);

  const handleCreateRoom = async (roomName: string) => {
    try {
      const response = await createRoom({
        body: { name: roomName },
      });
      refetch();
    } catch (error) {
      console.error("Failed to create room:", error);
      toast.error("Failed to create room");
    }
  };

  const { mutateAsync: deleteRoom } = useMutation("delete", "/api/auth/room/{id}", undefined);

  const handleDeleteRoom = async (room: Room) => {
    try {
      // Send JSON body as specified in OpenAPI
      const response = await deleteRoom({
        params: {
          path: {
            id: room.id,
          },
        },
      });

      refetch();
    } catch (error) {
      // Handle 401, 500, or other errors
      console.error("Failed to delete room:", error);
    }
  };

  const { mutateAsync: updateRoom } = useMutation("put", "/api/auth/room/{id}", undefined);

  const handleUpdateRoom = async (room: Room, e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    try {
      const formData = new FormData(e.currentTarget);
      const roomName = formData.get("name") as string;

      if (!roomName) {
        toast.error("Provide room name");
        return;
      }

      await updateRoom({
        body: { name: roomName },
        params: {
          path: {
            id: room.id,
          },
        },
      });

      setIsUpdateDialogOpen(false);
      setSelectedRoom(null);
      refetch();
      toast.success("Room renamed successfully");
    } catch (error) {
      // Handle 401, 500, or other errors
      console.error("Failed to update room:", error);
      toast.error("Failed to rename room");
    }
  };

  const handleJoinRoom = useCallback(
    async (room: Room) => {
      try {
        const tokens = await getRoomTokens({
          params: {
            path: {
              id: room.id,
            },
          },
        });
        if (!tokens) {
          toast.error("Error joining room");
          return;
        }
        sounds.callAccepted.play();
        setCallTokens({
          ...tokens,
          isRoomCall: true,
          timeStarted: new Date(),
          hasAudioEnabled: true,
          role: ParticipantRole.NONE,
          isRemoteControlEnabled: true,
          cameraTrackId: null,
          room: room,
        });
      } catch (error) {
        toast.error("Error joining room");
      }
    },
    [getRoomTokens],
  );

  useEffect(() => {
    if (searchQuery == "") {
      // Set rooms from the fetch response
      if (rooms) {
        setFilteredRooms(rooms);
      }
    } else {
      // Filter rooms based on search query
      if (rooms) {
        const filteredRooms = fuseSearch(rooms, searchQuery);
        setFilteredRooms(filteredRooms);
      }
    }
  }, [rooms, searchQuery]);

  callTokens?.audioToken;
  const isRoomCall = !(callTokens == null || (callTokens !== null && !callTokens.room));

  return (
    <div className="flex flex-col items-start gap-1.5 p-2">
      {isRoomCall && callTokens.room && <SelectedRoom room={callTokens.room} />}
      {!isRoomCall && (
        <div className="flex flex-col gap-2 w-full">
          <div className="flex items-center gap-2 w-full">
            <div className="relative flex-1">
              <HiMagnifyingGlass className="absolute left-2 top-1/2 transform -translate-y-1/2 text-gray-500 size-4" />
              <Input
                type="text"
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Search rooms"
                className="pl-8 w-full focus-visible:ring-opacity-20 focus-visible:ring-2 focus-visible:ring-blue-300"
              />
            </div>
            <Dialog open={isCreateDialogOpen} onOpenChange={setIsCreateDialogOpen}>
              <DialogTrigger asChild>
                <Button variant="outline" size="icon">
                  <Plus className="size-4 text-slate-500" />
                </Button>
              </DialogTrigger>
              <DialogContent className="max-w-[80%]" container={document.getElementById("app-body")}>
                <DialogHeader>
                  <DialogTitle>Create new room</DialogTitle>
                  <DialogDescription>Create a new room for your team to collaborate on.</DialogDescription>
                </DialogHeader>
                <div className="grid gap-3">
                  <Label htmlFor="room-name">Room name</Label>
                  <Input id="room-name" name="roomName" placeholder="Watercooler" />
                </div>
                <DialogDescription>Anyone in your team can modify or remove this room.</DialogDescription>
                <DialogFooter>
                  <DialogClose asChild>
                    <Button
                      onClick={() => {
                        const input = document.getElementById("room-name") as HTMLInputElement;
                        handleCreateRoom(input?.value || "");
                        setIsCreateDialogOpen(false);
                      }}
                      className="text-xs"
                    >
                      Create room
                    </Button>
                  </DialogClose>
                </DialogFooter>
              </DialogContent>
            </Dialog>
          </div>
          {filteredRooms && filteredRooms.length > 0 ?
            <div className="grid grid-cols-2 gap-2 w-full">
              {filteredRooms?.map((room) => (
                <RoomButton
                  onClick={() => handleJoinRoom(room)}
                  size="unsized"
                  title={room.name}
                  className="flex-1 min-w-0 text-slate-600"
                  cornerIcon={
                    <DropdownMenu>
                      <DropdownMenuTrigger className="hover:outline-solid hover:outline-1 hover:outline-slate-300 focus:ring-0 focus-visible:ring-0 hover:bg-slate-200 size-4 rounded-xs p-0 border-0 shadow-none hover:shadow-xs m-0 flex flex-row justify-center items-center">
                        <MoreHorizontal className="size-3 m-0" />
                      </DropdownMenuTrigger>
                      <DropdownMenuContent className="muted" align="end">
                        <DropdownMenuItem
                          className="text-xs [&>svg]:size-3.5"
                          onClick={() => {
                            setSelectedRoom(room);
                            setIsUpdateDialogOpen(true);
                          }}
                        >
                          <HiOutlinePencil />
                          Rename room
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          className="text-xs [&>svg]:size-3.5 text-red-600"
                          onClick={() => {
                            setSelectedRoom(room);
                            setIsDeleteDialogOpen(true);
                          }}
                        >
                          <HiOutlineTrash />
                          Delete room
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  }
                />
              ))}
            </div>
          : <EmptyRoomsState onCreateRoomClick={() => setIsCreateDialogOpen(true)} />}
          <Dialog open={isDeleteDialogOpen} onOpenChange={setIsDeleteDialogOpen}>
            <DialogContent container={document.getElementById("app-body")}>
              <DialogHeader>
                <DialogTitle>Delete Room</DialogTitle>
                <DialogDescription>
                  Are you sure you want to delete this room? This action cannot be undone.
                </DialogDescription>
              </DialogHeader>
              <DialogFooter>
                <Button
                  variant="destructive"
                  onClick={() => {
                    // Handle delete logic here
                    if (selectedRoom) {
                      handleDeleteRoom(selectedRoom);
                      setIsDeleteDialogOpen(false);
                      setSelectedRoom(null);
                    }
                  }}
                >
                  Delete
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
          <Dialog open={isUpdateDialogOpen} onOpenChange={setIsUpdateDialogOpen}>
            <DialogContent container={document.getElementById("app-body")}>
              <form onSubmit={(e) => selectedRoom && handleUpdateRoom(selectedRoom, e)}>
                <DialogHeader>
                  <DialogTitle>Rename room</DialogTitle>
                </DialogHeader>
                <div className="grid gap-2">
                  <Input
                    id="room-name"
                    name="name"
                    className="text-xs text-slate-500"
                    defaultValue={selectedRoom?.name}
                  />
                </div>
                <DialogDescription className="mt-4 mb-2">
                  Anyone in your team can modify or remove this room.
                </DialogDescription>
                <DialogFooter>
                  <DialogClose asChild>
                    <Button type="submit" className="text-xs">
                      Update room
                    </Button>
                  </DialogClose>
                </DialogFooter>
              </form>
            </DialogContent>
          </Dialog>
        </div>
      )}
    </div>
  );
};

const EmptyRoomsState = ({ onCreateRoomClick }: { onCreateRoomClick: () => void }) => {
  return (
    <div className="flex flex-col items-center justify-center py-12 px-4 text-center">
      <img src={doorImage} alt="No rooms" className="size-38 mb-6" />
      <p className="text-xs text-slate-600 mb-6 max-w-sm leading-relaxed">
        Think of Rooms as permanent, named meeting spots. They're great for your team's regular get-togethers like daily
        stand-ups or mob programming sessions.
      </p>
      <div className="flex flex-col gap-2 items-center">
        <Button onClick={onCreateRoomClick} className="text-sm">
          Create room
        </Button>
        <a href="https://docs.hopp.so/rooms" target="_blank" className="text-xs text-slate-600">
          Read docs
        </a>
      </div>
    </div>
  );
};

const SelectedRoom = ({ room }: { room: Room }) => {
  const { useMutation } = useAPI();
  const participants = useParticipants();
  const { teammates, user } = useStore();
  const roomContext = useRoomContext();

  const { mutateAsync: getRoomAnonymous } = useMutation("get", "/api/auth/room/anonymous", undefined);

  const handleInviteAnonymousUser = useCallback(async () => {
    const redirectURL = await getRoomAnonymous({
      params: {
        query: {
          room_id: room.id,
        },
      },
    });
    if (!redirectURL || !redirectURL.redirect_url) {
      toast.error("Error generating link");
      return;
    }
    const link = `${BACKEND_URLS.BASE}${redirectURL.redirect_url}`;
    await writeText(link);
    toast.success("Link copied to clipboard");
  }, [getRoomAnonymous, room.id]);

  // Listen for participant connection events and play sound when someone joins
  useEffect(() => {
    const handleParticipantConnected = (participant: any) => {
      // Filter out video/camera tracks to only play sound for actual users
      if (!participant.identity.includes("video") && !participant.identity.includes("camera")) {
        sounds.callAccepted.play();
      }
    };

    // Add event listener for participant connections
    roomContext.on(RoomEvent.ParticipantConnected, handleParticipantConnected);

    // Cleanup event listener on component unmount
    return () => {
      roomContext.off(RoomEvent.ParticipantConnected, handleParticipantConnected);
    };
  }, [roomContext]);

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
          <h3 className="small">{room.name}</h3>
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
