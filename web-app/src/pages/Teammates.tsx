import { useAPI } from "@/hooks/useQueryClients";
import { useHoppStore } from "@/store/store";
import { HoppAvatar } from "@/components/ui/hopp-avatar";
import { Badge } from "@/components/ui/badge";
import { toast } from "react-hot-toast";
import { RemoveTeammateDialog } from "@/components/RemoveTeammateDialog";

export function Teammates() {
  const { useQuery, useMutation } = useAPI();
  const authToken = useHoppStore((store) => store.authToken);

  const { data: user } = useQuery("get", "/api/auth/user", undefined, {
    queryHash: `user-${authToken}`,
    select: (data) => data,
  });

  const { data: teammates, refetch: refetchTeammates } = useQuery("get", "/api/auth/teammates", undefined, {
    queryHash: `teammates-${authToken}`,
    select: (data) => data,
  });

  const removeTeammateMutation = useMutation("delete", "/api/auth/teammates/{userId}");

  const handleRemoveTeammate = async (teammateId: string) => {
    try {
      await removeTeammateMutation.mutateAsync({
        params: { path: { userId: teammateId } },
      });
      await refetchTeammates();
      toast.success("Teammate removed successfully");
    } catch (error) {
      console.error("Failed to remove teammate:", error);
      toast.error("Failed to remove teammate");
    }
  };

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
            <div className="flex flex-col flex-1">
              <div className="flex items-center gap-2">
                <span className="font-medium">
                  {member.first_name} {member.last_name}
                </span>
              </div>
              <span className="text-sm text-muted-foreground">{member.email}</span>
            </div>
            <div className="flex items-center gap-2">
              {member.is_admin && <Badge variant="secondary">Admin</Badge>}
              {member.id !== user?.id && user?.is_admin && (
                <RemoveTeammateDialog
                  teammate={member}
                  onRemove={handleRemoveTeammate}
                  isPending={removeTeammateMutation.isPending}
                />
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
