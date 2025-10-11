import { cn } from "@/lib/utils";
import { FaSlack, FaGoogle } from "react-icons/fa";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import Logo from "@/assets/Hopp.png";
import LoginScreen from "@/assets/LoginScreen.png";

import { BACKEND_URLS } from "@/constants";
import { useEffect, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";
import { useHoppStore } from "@/store/store";
import { toast } from "react-hot-toast";
import { useCookies } from "react-cookie";
import { CgSpinner } from "react-icons/cg";
import { useAPI } from "@/hooks/useQueryClients";
import { AnimatedTooltip } from "@/components/ui/animated-tooltip";

const people = [
  {
    id: 1,
    name: "Umair M.",
    designation: "Software Engineer @ SAP",
    image: "https://gethopp.app/images/testimonials/Umair.jpg",
    quote: (
      <div>
        <b>The screen pairing quality is unmatched and crystal clear</b>, far better than any alternative. It's rare to
        see such a polished product launch, and I genuinely recommend it.
      </div>
    ),
  },
  {
    id: 2,
    name: "Markus Schicker",
    designation: "CIO at Outbank GmbH",
    image: "https://gethopp.app/images/testimonials/Markus.jpg",
    quote: (
      <div>
        As we do pair programming a lot and had very bad experiences with other "standard" tools regarding lagging and
        screen sharing quality, Hopp really rescued us. Fast, reliable, superb quality without any lags.
      </div>
    ),
  },
  {
    id: 3,
    name: "Vivek Vardhan",
    designation: "CEO & Co-Founder at Tryft.io",
    image: "https://gethopp.app/images/testimonials/Vivek_Vardhan.jpeg",
    quote: <div>P.S. Hopp is brilliant and we absolutely love it 😊</div>,
  },
];

interface AuthResponse {
  token: string;
  message?: string;
}

interface LoginFormProps extends React.ComponentPropsWithoutRef<"div"> {
  isInvitation?: boolean;
}

export function LoginForm({ className, isInvitation = false, ...props }: LoginFormProps) {
  const navigate = useNavigate();
  const { uuid } = useParams<{ uuid: string }>();
  const { useQuery } = useAPI();
  const [cookies, setCookie, removeCookie] = useCookies(["redirect_to_app"], {
    doNotParse: true,
  });
  const setAuthToken = useHoppStore((state) => state.setAuthToken);
  const [searchParams] = useSearchParams();
  const [isSignUp, setIsSignUp] = useState(isInvitation);
  const [formData, setFormData] = useState({
    email: "",
    password: "",
    firstName: "",
    lastName: "",
    teamName: "",
    teamInviteUUID: uuid || "",
  });
  const [isLoading, setIsLoading] = useState(false);

  const {
    data: invitationDetails,
    error: invitationError,
    isLoading: isLoadingInvitation,
  } = useQuery(
    "get",
    "/api/invitation-details/{uuid}",
    {
      params: {
        path: {
          uuid: uuid || "",
        },
      },
    },
    {
      enabled: isInvitation && uuid !== undefined,
      select: (data) => data,
    },
  );

  useEffect(() => {
    if (invitationError) {
      toast.error("Failed to fetch invitation details, contact your admin for a new invitation link");
      navigate("/login");
    }
  }, [invitationError, navigate]);

  useEffect(() => {
    // This flag will be present if the user is redirected from the app
    // and needs to login first to get a token
    const redirectToApp = searchParams.get("redirect_to_app");
    if (redirectToApp) {
      setCookie("redirect_to_app", true, {
        expires: new Date(Date.now() + 1000 * 60 * 15), // 15 minutes
      });
    }

    // This will be visible on a callback from social auth
    const token = searchParams.get("token");
    if (token) {
      setAuthToken(token);

      // If the user should redirect to the app, we need to remove the cookie
      if (cookies.redirect_to_app) {
        removeCookie("redirect_to_app");
        // Allow app to initialize after auth token is set
        // and then redirect. Probably holding this in state would be more appropriate but it can work for now
        setTimeout(() => {
          window.open(`hopp:///authenticate?token=${token}`, "_blank");
          navigate("/dashboard?show_app_token_banner=true");
        }, 500);
      } else {
        navigate("/");
      }
    }
  }, [searchParams, navigate, setAuthToken, setCookie, removeCookie, cookies]);

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setFormData({
      ...formData,
      [e.target.id]: e.target.value,
    });
  };

  const handleEmailAuth = async (e: React.FormEvent) => {
    e.preventDefault();
    setIsLoading(true);

    try {
      const endpoint = isSignUp ? "/api/sign-up" : "/api/sign-in";
      const response = await fetch(`${BACKEND_URLS.BASE}${endpoint}`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          email: formData.email,
          password: formData.password,
          ...(isSignUp && {
            first_name: formData.firstName,
            last_name: formData.lastName,
            ...(formData.teamInviteUUID ?
              { team_invite_uuid: formData.teamInviteUUID }
            : { team_name: formData.teamName }),
          }),
        }),
      });

      const data = (await response.json()) as AuthResponse;

      if (!response.ok) {
        throw new Error(data.message || "Authentication failed");
      }

      if (data.token) {
        setAuthToken(data.token);

        // If the user should redirect to the app, we need to remove the cookie
        if (cookies.redirect_to_app) {
          removeCookie("redirect_to_app");
          window.open(`hopp:///authenticate?token=${data.token}`, "_blank");
          navigate("/dashboard");
        } else {
          navigate("/dashboard");
        }
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : "Authentication failed";
      toast.error(message);
    } finally {
      setIsLoading(false);
    }
  };

  const handleSlackLogin = () => {
    const url = new URL(`${BACKEND_URLS.BASE}/api/auth/social/slack`);
    if (formData.teamInviteUUID) {
      url.searchParams.set("invite_uuid", formData.teamInviteUUID);
    }
    window.location.href = url.toString();
  };

  const handleGoogleLogin = () => {
    const url = new URL(`${BACKEND_URLS.BASE}/api/auth/social/google`);
    if (formData.teamInviteUUID) {
      url.searchParams.set("invite_uuid", formData.teamInviteUUID);
    }
    window.location.href = url.toString();
  };

  if (isLoadingInvitation) {
    return (
      <div className="flex flex-row items-center justify-center min-w-screen min-h-screen">
        <div className="flex flex-row items-center gap-2">
          <CgSpinner className="size-5 animate-spin" />
          <p>Loading invitation...</p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-w-screen min-h-screen overflow-clip">
      <div className="flex flex-col items-center justify-center w-full xl:w-1/2 min-h-screen bg-white p-8">
        <img src={Logo} alt="Logo" className="h-12 w-auto mb-8" />
        <div className={cn("flex flex-col gap-6 w-full max-w-md", className)} {...props}>
          <Card className="w-full">
            <CardHeader className="text-center">
              <CardTitle className="text-xl">
                {isInvitation && invitationDetails ?
                  `Join ${invitationDetails.name} team on Hopp`
                : isSignUp ?
                  "Create an account"
                : "Welcome back"}
              </CardTitle>
              <CardDescription>
                {isInvitation && invitationDetails ?
                  "Sign up to join your team"
                : isSignUp ?
                  "Sign up for a new account"
                : "Login with your email or social account"}
              </CardDescription>
            </CardHeader>
            <CardContent>
              <form onSubmit={handleEmailAuth}>
                <div className="grid gap-6">
                  <div className="flex flex-col gap-4">
                    <Button type="button" variant="outline" className="w-full" onClick={handleSlackLogin}>
                      <FaSlack className="size-5 mr-2" />
                      {isSignUp ? "Sign up with Slack" : "Login with Slack"}
                    </Button>
                    <Button type="button" variant="outline" className="w-full" onClick={handleGoogleLogin}>
                      <FaGoogle className="size-5 mr-2" />
                      {isSignUp ? "Sign up with Google" : "Login with Google"}
                    </Button>
                  </div>
                  {!isSignUp && (
                    <div className="relative text-center text-sm after:absolute after:inset-0 after:top-1/2 after:z-0 after:flex after:items-center after:border-t after:border-border">
                      <span className="relative z-10 bg-background px-2 text-muted-foreground">
                        Or continue with email
                      </span>
                    </div>
                  )}
                  <div className="grid gap-4">
                    {isSignUp && (
                      <>
                        <div className="grid gap-2">
                          <Label htmlFor="firstName">First Name</Label>
                          <Input id="firstName" value={formData.firstName} onChange={handleInputChange} required />
                        </div>
                        <div className="grid gap-2">
                          <Label htmlFor="lastName">Last Name</Label>
                          <Input id="lastName" value={formData.lastName} onChange={handleInputChange} required />
                        </div>
                        {!isInvitation && (
                          <div className="grid gap-2">
                            <Label htmlFor="teamName">Team Name</Label>
                            <Input id="teamName" value={formData.teamName} onChange={handleInputChange} required />
                          </div>
                        )}
                      </>
                    )}
                    <div className="grid gap-2">
                      <Label htmlFor="email">Email</Label>
                      <Input
                        id="email"
                        type="email"
                        value={formData.email}
                        onChange={handleInputChange}
                        placeholder="10x_engineer@unicorn.com"
                        required
                      />
                    </div>
                    <div className="grid gap-2">
                      <div className="flex items-center">
                        <Label htmlFor="password">Password</Label>
                        {!isSignUp && (
                          <a href="#" className="ml-auto text-sm underline-offset-4 hover:underline">
                            Forgot your password?
                          </a>
                        )}
                      </div>
                      <Input
                        id="password"
                        type="password"
                        value={formData.password}
                        onChange={handleInputChange}
                        required
                      />
                    </div>
                    <Button type="submit" className="w-full" disabled={isLoading}>
                      {isLoading ?
                        "Loading..."
                      : isSignUp ?
                        "Sign Up"
                      : "Sign In"}
                    </Button>
                  </div>
                  {!isInvitation && (
                    <div className="text-center text-sm">
                      {isSignUp ?
                        <>
                          Already have an account?{" "}
                          <button
                            type="button"
                            onClick={() => setIsSignUp(false)}
                            className="text-primary underline underline-offset-4"
                          >
                            Sign in
                          </button>
                        </>
                      : <>
                          Don&apos;t have an account?{" "}
                          <button
                            type="button"
                            onClick={() => setIsSignUp(true)}
                            className="text-primary underline underline-offset-4"
                          >
                            Sign up
                          </button>
                        </>
                      }
                    </div>
                  )}
                </div>
              </form>
            </CardContent>
          </Card>
        </div>
      </div>

      <div className="hidden xl:flex w-1/2 h-screen max-h-[99.8vh] bg-white flex-col justify-center items-center p-2 relative overflow-visible">
        <div className="w-full h-full p-16 bg-[#5625E7] flex flex-col rounded-3xl">
          <div className="relative z-10 text-center">
            <h1 className="text-4xl font-medium text-left text-white mb-6 leading-tight">
              Remote teams start to love <br /> pair-programming again
            </h1>
            <p className="text-2xl text-left font-light text-purple-100 mb-12 leading-tight opacity-70 max-w-[70%]">
              Low-latency, crisp quality and everything you need to stay in the flow while pairing
            </p>

            <div className="mb-8 relative w-full h-[350px]">
              <img
                src={LoginScreen}
                alt="Hopp Login Interface"
                className="absolute w-[110%] h-auto max-w-md mx-auto top-1/2 left-1/2 transform -translate-x-1/2 -translate-y-1/2 min-w-[135%] overflow-visible"
              />
            </div>

            <div className="mt-18">
              <p className="text-purple-200 text-sm mb-4">What engineers say about Hopp</p>

              <div className="flex justify-center items-center gap-0">
                <AnimatedTooltip items={people} />
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
