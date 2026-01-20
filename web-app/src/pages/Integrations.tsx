import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { CustomIcons } from "@/components/ui/icons";
import { Loader2 } from "lucide-react";
import { useHoppStore } from "@/store/store";
import toast from "react-hot-toast";
import { BACKEND_URLS } from "@/constants";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import type { components } from "@/openapi";

export function Integrations() {
  const authToken = useHoppStore((state) => state.authToken);
  const queryClient = useQueryClient();

  // Fetch current Slack installation for the user's team
  const { data: slackInstallation } = useQuery<components["schemas"]["SlackInstallation"] | null>({
    queryKey: ["slack-installation"],
    queryFn: async () => {
      const response = await fetch(`${BACKEND_URLS.BASE}/api/auth/slack/installation`, {
        headers: {
          Authorization: `Bearer ${authToken}`,
        },
      });
      if (response.status === 404) {
        return null;
      }
      if (!response.ok) {
        throw new Error("Failed to fetch Slack installation");
      }
      return response.json();
    },
    enabled: !!authToken,
  });

  // Delete Slack installation mutation
  const deleteSlackMutation = useMutation({
    mutationFn: async () => {
      const response = await fetch(`${BACKEND_URLS.BASE}/api/auth/slack/installation`, {
        method: "DELETE",
        headers: {
          Authorization: `Bearer ${authToken}`,
        },
      });
      if (!response.ok) {
        throw new Error("Failed to delete Slack installation");
      }
    },
    onSuccess: () => {
      toast.success("Slack integration removed successfully");
      queryClient.invalidateQueries({ queryKey: ["slack-installation"] });
    },
    onError: (error: Error) => {
      toast.error(`Failed to remove Slack integration: ${error.message}`);
    },
  });

  const handleDeleteSlack = () => {
    if (
      window.confirm(
        "Are you sure you want to remove the Slack integration? Your team will no longer be able to use /hopp in Slack.",
      )
    ) {
      deleteSlackMutation.mutate();
    }
  };

  const handleInstallSlack = () => {
    // Authenticated endpoint - pass token as query param for redirect
    window.location.href = `${BACKEND_URLS.BASE}/api/auth/slack/install?token=${authToken}`;
  };

  return (
    <div className="">
      <div className="mb-6">
        <h2 className="h2-section">Integrations and connected apps</h2>
      </div>

      <div className="flex flex-col md:flex-row gap-4 flex-wrap w-full">
        {/* Slack Integration Card */}
        <Card className="p-6 w-[450px]">
          <div className="flex items-start gap-4">
            <CustomIcons.Slack className="h-10 w-10 shrink-0" />
            <div className="flex-1 space-y-1">
              <div className="flex items-center justify-between">
                <h3 className="text-lg font-semibold">Slack</h3>
                {slackInstallation ?
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleDeleteSlack}
                    disabled={deleteSlackMutation.isPending}
                    className="text-destructive hover:text-destructive"
                  >
                    {deleteSlackMutation.isPending ?
                      <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                    : null}
                    Remove Integration
                  </Button>
                : <Button size="sm" onClick={handleInstallSlack} variant="outline">
                    Install
                  </Button>
                }
              </div>
            </div>
          </div>
          <CardContent className="p-0 pt-4">
            <p className="text-muted-foreground">
              Start pairing sessions directly from Slack with{" "}
              <code className="bg-muted px-1.5 py-0.5 rounded text-sm">/hopp</code>
            </p>
          </CardContent>
        </Card>

        <Card className="p-6 w-[450px]">
          <div className="flex items-start gap-4">
            <CustomIcons.MSTeams className="h-10 w-10 shrink-0" />
            <div className="flex-1 space-y-1">
              <div className="flex items-center justify-between">
                <h3 className="text-lg font-semibold">Microsoft Teams</h3>
                <Button disabled variant="outline">
                  Coming soon
                </Button>
              </div>
            </div>
          </div>

          <CardContent className="p-0 pt-4">
            <p className="text-muted-foreground">
              Start pairing sessions directly from Microsoft Teams with{" "}
              <code className="bg-muted px-1.5 py-0.5 rounded text-sm">/hopp</code>
            </p>
          </CardContent>
        </Card>

        <Card className="p-6 w-[450px]">
          <div className="flex items-start gap-4">
            <CustomIcons.Raycast className="h-10 w-10 shrink-0" />
            <div className="flex-1 space-y-1">
              <div className="flex items-center justify-between">
                <h3 className="text-lg font-semibold">Raycast</h3>
                <Button disabled variant="outline">
                  Coming soon
                </Button>
              </div>
            </div>
          </div>

          <CardContent className="p-0 pt-4">
            <p className="text-muted-foreground">
              Use Raycast shortcuts to start pairing sessions from your favorite launcher.
            </p>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
