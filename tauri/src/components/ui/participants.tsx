import { components } from "@/openapi";
import { ParticipantRow } from "./participant-row-wo-livekit";
import { ScrollArea } from "./scroll-area";
import { useState, useEffect } from "react";
import Fuse from "fuse.js";
import { Input } from "./input";
import { HiMagnifyingGlass } from "react-icons/hi2";

interface ParticipantsProps {
  teammates: components["schemas"]["BaseUser"][];
}

const fuseSearch = (teammates: components["schemas"]["BaseUser"][], searchQuery: string) => {
  const fuse = new Fuse(teammates, {
    keys: ["first_name", "last_name"],
    threshold: 0.3,
    shouldSort: true,
  });
  return fuse.search(searchQuery).map((result) => result.item);
};

export const Participants = ({ teammates }: ParticipantsProps) => {
  const [searchQuery, setSearchQuery] = useState("");

  const [filteredTeammates, setFilteredTeammates] = useState(teammates);

  useEffect(() => {
    if (searchQuery === "") {
      setFilteredTeammates(teammates);
      return;
    }
    const filteredTeammates = fuseSearch(teammates, searchQuery);
    setFilteredTeammates(filteredTeammates);
  }, [teammates, searchQuery]);

  const onlineTeammates = filteredTeammates?.filter((teammate) => teammate.is_active) || [];
  const offlineTeammates = filteredTeammates?.filter((teammate) => !teammate.is_active) || [];

  return (
    <div className="flex flex-col gap-2 w-full">
      <div className="relative" style={{ width: "99%" }}>
        <HiMagnifyingGlass className="absolute left-2 top-1/2 transform -translate-y-1/2 text-gray-500 size-4" />
        <Input
          type="text"
          placeholder="Search teammates..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          className="pl-8 w-full focus-visible:ring-opacity-20 focus-visible:ring-2 focus-visible:ring-blue-300"
        />
      </div>

      <div className="">
        <h3 className="muted text-xs text-slate-500 font-medium mb-2">Online ({onlineTeammates.length})</h3>
        <div className="flex flex-col gap-2">
          {onlineTeammates.map((teammate) => (
            <ParticipantRow key={teammate.id} user={teammate} />
          ))}
        </div>
      </div>

      <div>
        <h3 className="muted text-xs text-slate-500 font-medium my-2">Offline ({offlineTeammates.length})</h3>
        <ScrollArea className="max-h-full overflow-y-auto mb-4">
          <div className="flex flex-col gap-2">
            {offlineTeammates.map((teammate) => (
              <ParticipantRow key={teammate.id} user={teammate} />
            ))}
          </div>
        </ScrollArea>
      </div>
    </div>
  );
};
