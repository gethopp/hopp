import { useAPI, isFetchError } from "@/hooks/useQueryClients";
import { useHoppStore } from "@/store/store";
import { FiEdit } from "react-icons/fi";
import { Button } from "@/components/ui/button";
import { HoppAvatar } from "@/components/ui/hopp-avatar";
import { useNavigate, useSearchParams } from "react-router";
import CopyButton from "@/components/ui/copy-button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { toast } from "react-hot-toast";
import { BACKEND_URLS } from "@/constants";
import { useEffect, useState, useRef, useCallback } from "react";
import { BsApple, BsWindows } from "react-icons/bs";
import { VscTerminalLinux } from "react-icons/vsc";
import { z } from "zod";
import CreatableSelect from "react-select/creatable";
import { SignInSuccessModal } from "@/components/SignInSuccessModal";
import { AuthenticationDialog } from "@/components/AuthenticationDialog";
import { usePostHog } from "posthog-js/react";
import { queryClient } from "@/App";
import { WindowsDownloadModal } from "@/components/WindowsDownloadModal";

// Create email validation schema using zod
const emailSchema = z.string().email("Invalid email format");

interface GitHubRelease {
  tag_name: string;
  assets: Array<{
    name: string;
    browser_download_url: string;
  }>;
}

// Interface for email option type
interface EmailOption {
  value: string;
  label: string;
}

type DownloadSystem = "MACOS_INTEL" | "MACOS_APPLE_SILICON" | "WINDOWS" | "LINUX";

const ErrorMessages: Record<number, string> = {
  409: "You cannot change teams while you have teammates. Please contact support for assistance.",
  404: "The invitation link is invalid or has expired. Please contact the team admin for a new invitation.",
};

/**
 * This can happen in cases that GitHub API returns an error
 */
const ReleaseLinkNotFound = ({ toastId }: { toastId: string }) => (
  <span className="flex flex-col gap-2 items-start">
    <div className="">Download link not found for your selection. Please check the release page.</div>
    <Button
      variant="outline"
      onClick={() => {
        window.open("https://github.com/gethopp/hopp-releases/releases", "_blank");
        toast.dismiss(toastId);
      }}
    >
      Open Releases
    </Button>
  </span>
);

export function Dashboard() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const [showAuthDialog, setShowAuthDialog] = useState(searchParams.get("show_app_token_banner") === "true");
  const inviteParam = searchParams.get("invite");

  const [latestRelease, setLatestRelease] = useState<GitHubRelease | null>(null);
  // Updated state for react-select
  const [emailOptions, setEmailOptions] = useState<EmailOption[]>([]);
  const [emailError, setEmailError] = useState<string | null>(null);
  const posthog = usePostHog();

  // Function to handle file downloads
  const downloadFile = (system: DownloadSystem) => {
    if (system === "LINUX") {
      posthog.capture("app_download_attempted", {
        platform: "linux",
        download_type: "notification_signup",
      });
      window.open("https://forms.gle/Fce4jTsDGzKVimib6", "_blank");
      return;
    }

    if (!latestRelease) {
      toast.error("Release information not yet loaded. Please wait a moment and try again.");
      return;
    }

    let downloadUrl: string | undefined;
    let platformName: string;

    switch (system) {
      case "MACOS_INTEL": {
        const intelAsset = latestRelease.assets.find((asset) => asset.name.endsWith("_x64.dmg"));
        downloadUrl = intelAsset?.browser_download_url;
        platformName = "macos_intel";
        break;
      }
      case "MACOS_APPLE_SILICON": {
        const appleAsset = latestRelease.assets.find((asset) => asset.name.endsWith("_aarch64.dmg"));
        downloadUrl = appleAsset?.browser_download_url;
        platformName = "macos_apple_silicon";
        break;
      }
      case "WINDOWS": {
        const windowsAsset = latestRelease.assets.find((asset) => asset.name.endsWith(".msi.zip"));
        downloadUrl = windowsAsset?.browser_download_url;
        platformName = "windows";
        break;
      }
    }

    if (downloadUrl) {
      posthog.capture("app_download_attempted", {
        platform: platformName,
        download_type: "direct_download",
      });

      const link = document.createElement("a");
      link.href = downloadUrl;
      link.setAttribute("download", "");
      document.body.appendChild(link);
      link.click();
      document.body.removeChild(link);
      toast.success("Download started!");
    } else {
      posthog.capture("app_download_failed", {
        platform: platformName,
        error_reason: "download_url_not_found",
      });

      /**
       * This can happen in cases that GitHub API returns an error
       */
      toast((t) => <ReleaseLinkNotFound toastId={t.id} />, {
        duration: Infinity,
      });
    }
  };

  useEffect(() => {
    const fetchLatestRelease = async () => {
      try {
        const response = await fetch("https://api.github.com/repos/gethopp/hopp-releases/releases/latest");
        if (!response.ok) throw new Error("Failed to fetch latest release");
        const data = await response.json();
        setLatestRelease(data);
      } catch (error) {
        console.error("Error fetching latest release:", error);
        const fallbackRelease: GitHubRelease = {
          tag_name: "latest",
          assets: [
            {
              name: "hopp_x64.dmg",
              browser_download_url:
                "https://github.com/gethopp/hopp-releases/releases/latest/download/hopp.app.x64.tar.gz",
            },
            {
              name: "hopp_aarch64.dmg",
              browser_download_url:
                "https://github.com/gethopp/hopp-releases/releases/latest/download/hopp.app.aarch64.tar.gz",
            },
            {
              name: "hopp.msi.zip",
              browser_download_url:
                "https://github.com/gethopp/hopp-releases/releases/latest/download/hopp_x64_en-US.msi.zip",
            },
          ],
        };
        setLatestRelease(fallbackRelease);
      }
    };

    fetchLatestRelease();
  }, []);

  const { useQuery, useMutation } = useAPI();
  const authToken = useHoppStore((store) => store.authToken);

  // TODO: Combine useQueries into one, and merge
  // the store from individual useState
  const { data: user } = useQuery("get", "/api/auth/user", undefined, {
    queryHash: `user-${authToken}`,
    select: (data) => data,
  });

  const { data: teammates } = useQuery("get", "/api/auth/teammates", undefined, {
    queryHash: `teammates-${authToken}`,
    select: (data) => data,
  });

  const { data: inviteData } = useQuery("get", "/api/auth/get-invite-uuid", undefined, {
    queryHash: `invite-${authToken}`,
    select: (data) => data,
  });

  const { data: appAuthToken } = useQuery("get", "/api/auth/authenticate-app", undefined, {
    queryHash: `token-app-${authToken}`,
    select: (data) => data.token,
  });

  const { mutateAsync: inviteTeammates, isPending: isInviting } = useMutation("post", "/api/auth/send-team-invites");

  const { mutateAsync: changeTeam } = useMutation("post", "/api/auth/change-team/{uuid}", {
    retry: false,
  });

  const inviteUrl = inviteData?.invite_uuid ? `${BACKEND_URLS.BASE}/invitation/${inviteData.invite_uuid}` : "";

  // Ref to track if team invite has been processed to prevent double execution in Strict Mode
  const inviteProcessedRef = useRef<string | null>(null);

  // Handle team change invitation
  useEffect(() => {
    const handleTeamInvite = async () => {
      console.log("Executing handleTeamInvite");
      if (!inviteParam) return;

      // Prevent double execution in React Strict Mode
      if (inviteProcessedRef.current === inviteParam) {
        console.log("Invite already processed, skipping");
        return;
      }

      inviteProcessedRef.current = inviteParam;

      try {
        const result = await changeTeam({
          params: {
            path: {
              uuid: inviteParam,
            },
          },
        });

        if (result && result.team_name) {
          toast.success(`Successfully joined team: ${result.team_name}`);
          // Remove the invite parameter from URL
          const newSearchParams = new URLSearchParams(searchParams);
          newSearchParams.delete("invite");
          navigate(`?${newSearchParams.toString()}`, { replace: true });
          // Invalidate all cached queries
          queryClient.invalidateQueries();
          queryClient.clear();
        }
      } catch (error: unknown) {
        if (isFetchError(error)) {
          const message =
            ErrorMessages[error.response.status] || "Failed to join the team. Contact us if the problem persists.";
          toast.error(message);
        } else {
          toast.error("Failed to join the team. Contact us if the problem persists.");
        }

        // Remove the invalid invite parameter from URL
        const newSearchParams = new URLSearchParams(searchParams);
        newSearchParams.delete("invite");
        navigate("/dashboard");
      }
    };

    handleTeamInvite();
  }, [inviteParam, searchParams, navigate, changeTeam]);

  // Validate email format
  const validateEmail = (email: string): boolean => {
    try {
      emailSchema.parse(email);
      return true;
    } catch {
      return false;
    }
  };

  // Handle creation of new email option
  const handleCreateOption = (inputValue: string) => {
    setEmailError(null);

    // Validate email
    if (!validateEmail(inputValue)) {
      setEmailError("Invalid email format");
      return;
    }

    // Check for duplicates
    if (emailOptions.some((option) => option.value === inputValue)) {
      setEmailError("Email already added");
      return;
    }

    const newOption = { value: inputValue, label: inputValue };
    setEmailOptions([...emailOptions, newOption]);
  };

  // Handle invite users
  const handleInviteUsers = async () => {
    if (emailOptions.length === 0) {
      toast.error("Please add at least one email to invite");
      return;
    }

    try {
      const emails = emailOptions.map((option) => option.value);
      await inviteTeammates({
        body: {
          invitees: emails,
        },
      });

      toast.success(`Invitation sent to ${emails.length} email(s)`);
      setEmailOptions([]);
    } catch (error) {
      // TODO: https://github.com/openapi-ts/openapi-typescript/issues/2317
      toast.error("Limit reached, please try inviting your teammates again in a few hours");
      console.error(error);
    }
  };

  const onAuthenticationDialogOpenChange = useCallback(() => {
    setShowAuthDialog(false);
    // Remove `show_app_token_banner=true` from the URL
    const newSearchParams = new URLSearchParams(searchParams);
    newSearchParams.delete("show_app_token_banner");
    navigate(`?${newSearchParams.toString()}`, { replace: true });
  }, [searchParams, navigate]);

  return (
    <div className="flex flex-col w-full">
      <SignInSuccessModal />
      <AuthenticationDialog
        open={showAuthDialog}
        onOpenChange={onAuthenticationDialogOpenChange}
        appAuthToken={appAuthToken}
      />

      <h2 className="h2-section min-w-full">Dashboard</h2>
      <div className="flex flex-col lg:flex-row lg:flex-wrap gap-4">
        <div className="flex flex-col lg:w-1/2 gap-4">
          <section aria-labelledby="teammates">
            <div className="flex flex-col gap-4">
              {/* Container with max-width matching the grid */}
              <div className="flex flex-row items-center justify-between max-w-sm">
                <h3 className="h3-subsection">Teammates</h3>
                {user?.is_admin && (
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={() => {
                      navigate("/teammates");
                    }}
                  >
                    <FiEdit />
                    <span className="sr-only">Edit teammates</span>
                  </Button>
                )}
              </div>
              <div className="flex flex-col gap-4">
                <div className="grid gap-3 md:[grid-template-columns:repeat(2,minmax(0,180px))] lg:[grid-template-columns:repeat(4,minmax(0,180px))]">
                  {teammates?.length === 0 && <span className="muted mx-1">No teammates yet</span>}
                  {teammates?.map((teammate) => (
                    <div
                      key={teammate.id}
                      className="flex flex-row items-center gap-2 w-fit hover:bg-muted/50 p-2 rounded-lg transition-colors"
                    >
                      <HoppAvatar
                        src={teammate.avatar_url || undefined}
                        firstName={teammate.first_name}
                        lastName={teammate.last_name}
                      />
                      <span className="font-medium truncate">
                        {teammate.first_name} {teammate.last_name.charAt(0)}.
                      </span>
                    </div>
                  ))}
                </div>
                <span className="muted ml-1">{teammates?.length || 0} team members</span>
              </div>
            </div>
          </section>

          <section aria-labelledby="team-invitation">
            <div className="flex flex-col gap-4">
              <div className="flex flex-row items-center gap-2">
                <h3 className="h3-subsection">Invite your teammates</h3>
              </div>
              <div className="space-y-2">
                <Label htmlFor="invite-url">Invite Link</Label>
                <p className="muted">
                  Share your team invite link to anyone via email, Slack, Microsoft Teams. <br />
                  Invitees will join your team as members.
                </p>
                <div className="flex items-center gap-2">
                  <Input id="invite-url" value={inviteUrl} disabled className="max-w-md" />
                  <CopyButton
                    onCopy={() => {
                      navigator.clipboard.writeText(inviteUrl);
                      toast.success("Invitation link copied to clipboard");
                    }}
                  />
                </div>
                <p className="muted">*Share this link with people you trust 🔐</p>
              </div>

              {/* Email invitation section with React-Select Creatable */}
              <div className="mt-4 space-y-2">
                <Label htmlFor="email-invite">Invite by Email</Label>
                <p className="muted">Enter email addresses to send invitations directly to your teammates</p>
                <div className="flex flex-col gap-2 max-w-md">
                  <CreatableSelect
                    id="email-invite"
                    isMulti
                    placeholder="Type email addresses and press enter..."
                    options={[]}
                    value={emailOptions}
                    onChange={(newValue) => setEmailOptions(newValue as EmailOption[])}
                    onCreateOption={handleCreateOption}
                    formatCreateLabel={(inputValue) => `Add "${inputValue}"`}
                    classNamePrefix="react-select"
                    className="react-select-container"
                    components={{
                      DropdownIndicator: () => null,
                      IndicatorSeparator: () => null,
                    }}
                    styles={{
                      control: (base) => ({
                        ...base,
                        fontSize: "12px",
                      }),
                    }}
                  />
                  {emailError && <p className="text-red-500 text-xs mt-1">{emailError}</p>}
                </div>

                {/* Invite button */}
                <div className="mt-4">
                  <Button
                    onClick={handleInviteUsers}
                    disabled={emailOptions.length === 0 || isInviting}
                    className="mt-2"
                  >
                    {isInviting ? "Sending Invites..." : "Send Invitations"}
                  </Button>
                </div>
              </div>
            </div>
          </section>
        </div>

        <section aria-labelledby="download-app" className="w-full lg:w-[calc(50%-2rem)]">
          <div className="flex flex-col gap-4">
            <h2 className="h3-subsection">Download the app</h2>
            <p className="small">Download options for different operating systems and architectures.</p>

            <div className="flex flex-row items-center justify-center gap-6">
              <BsApple className="size-4 text-slate-600" />
              <div className="flex flex-col">
                <span className="font-normal">macOS</span>
                <span className="muted">Intel & M series chips</span>
              </div>
              <div className="flex flex-row gap-2 flex-wrap ml-auto">
                <Button
                  variant="outline"
                  className="ml-auto"
                  onClick={() => downloadFile("MACOS_INTEL")}
                  disabled={!latestRelease}
                >
                  Intel Chip
                </Button>
                <Button
                  variant="outline"
                  className="ml-auto"
                  onClick={() => downloadFile("MACOS_APPLE_SILICON")}
                  disabled={!latestRelease}
                >
                  Apple Silicon
                </Button>
              </div>
            </div>
            <div className="flex flex-row items-center justify-center gap-6">
              <BsWindows className="size-4 text-slate-600" />
              <div className="flex flex-col">
                <span className="font-normal">Windows</span>
                <span className="muted">Windows 7 or later</span>
              </div>

              <WindowsDownloadModal
                onDownload={() => downloadFile("WINDOWS")}
                disabled={!latestRelease}
                triggerClassName="ml-auto"
              />
            </div>
            <div className="flex flex-row items-center justify-center gap-6">
              <VscTerminalLinux className="size-4 text-slate-600" />
              <div className="flex flex-col">
                <span className="font-normal">Linux</span>
                <span className="muted">Various Linux distributions</span>
              </div>
              <Button variant="outline" className="ml-auto" onClick={() => downloadFile("LINUX")}>
                Notify me
              </Button>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}
