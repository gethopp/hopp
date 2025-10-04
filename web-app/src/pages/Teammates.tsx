import { useAPI } from "@/hooks/useQueryClients";
import { useHoppStore } from "@/store/store";
import { HoppAvatar } from "@/components/ui/hopp-avatar";
import { Badge } from "@/components/ui/badge";

export function Teammates() {
  const { useQuery } = useAPI();
  const authToken = useHoppStore((store) => store.authToken);

  const { data: user } = useQuery("get", "/api/auth/user", undefined, {
    queryHash: `user-${authToken}`,
    select: (data) => data,
  });

  const { data: teammates } = useQuery("get", "/api/auth/teammates", undefined, {
    queryHash: `teammates-${authToken}`,
    select: (data) => data,
  });

  // Combine current user with teammates
  const allMembers = user ? [user, ...(teammates || [])] : teammates || [];

  return (
    <div className="flex flex-col w-full">
      <h2 className="h2-section">Team Members</h2>

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 pt-4">
        {allMembers.map((member) => (
          <div key={member.id} className="flex items-center gap-4 p-4 rounded-lg border bg-card">
            <HoppAvatar
              src={member.avatar_url || undefined}
              firstName={member.first_name}
              lastName={member.last_name}
            />
            <div className="flex flex-col">
              <div className="flex items-center gap-2">
                <span className="font-medium">
                  {member.first_name} {member.last_name}
                </span>
              </div>
              <span className="text-sm text-muted-foreground">{member.email}</span>
            </div>
            {member.is_admin && (
              <Badge className="ml-auto" variant="secondary">
                Admin
              </Badge>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
