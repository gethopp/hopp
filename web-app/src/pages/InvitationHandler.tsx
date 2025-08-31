import { useHoppStore } from "@/store/store";
import { LoginForm } from "./Login";
import { useNavigate, useParams } from "react-router";
import { useEffect } from "react";

export function InvitationHandler() {
  const authToken = useHoppStore((state) => state.authToken);
  const { uuid } = useParams<{ uuid: string }>();
  const navigate = useNavigate();

  useEffect(() => {
    if (authToken) {
      // Redirect to dashboard with an invite param
      navigate(`/dashboard?invite=${uuid}`);
    }
  }, [authToken, navigate, uuid]);

  if (!authToken) {
    return <LoginForm isInvitation={true} />;
  }

  return null;
}
