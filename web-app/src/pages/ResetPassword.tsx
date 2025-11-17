import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { toast } from "react-hot-toast";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import Logo from "@/assets/Hopp.png";
import { useParams } from "react-router-dom";
import { useAPI } from "@/hooks/useQueryClients";

export function ResetPassword() {
  const { useMutation } = useAPI();
  const [isFormSubmitted, setFormSubmitted] = useState(false);
  const [message, setMessage] = useState("");
  const { token } = useParams();

  const resetPasswordMutation = useMutation("patch", "/api/reset-password/{token}");

  const handleSubmit = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    try {
      const formData = new FormData(e.currentTarget);
      const extractFormData = Object.fromEntries(formData);

      if (extractFormData.password != extractFormData.reEnterPassword) {
        throw new Error("Passwords do not match. Please try again.");
      }
      if (!token) {
        throw new Error("Reset token is missing");
      }
      const data = await resetPasswordMutation.mutateAsync({
        params: {
          path: {
            token: token,
          },
        },
        body: {
          password: extractFormData.password as string,
        },
      });
      setMessage(data.message || "Your password has been changed. You can now use it to log in.");
      setFormSubmitted(true);
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : "Something went wrong, please try again.";
      toast.error(errorMessage);
    }
  };
  return (
    <div className="flex flex-col items-center justify-center w-screen h-screen">
      <div className="flex w-full h-full max-h-[1200px] max-w-[2500px] overflow-clip">
        <div className="flex flex-col items-center justify-center w-full max-w-lg mx-auto bg-white p-8">
          <img src={Logo} alt="Logo" className="h-12 w-auto mb-8" />
          <Card className="w-full">
            <CardHeader className="text-center">
              <CardTitle className="text-xl">{isFormSubmitted ? "Password changed!" : "Reset password"}</CardTitle>
              <CardDescription>{isFormSubmitted && message ? `${message}` : "Set your new password"}</CardDescription>
            </CardHeader>
            {isFormSubmitted ?
              <CardContent>
                <a href="/login" className="ml-auto text-sm underline-offset-4 hover:underline">
                  <Button type="submit" className="w-full">
                    Go back to Login
                  </Button>
                </a>
              </CardContent>
            : <CardContent>
                <form onSubmit={handleSubmit} className="space-y-4">
                  <div className="space-y-1">
                    <Label htmlFor="password">Password*</Label>
                    <Input required id="password" name="password" type="password" />
                    <Label htmlFor="reEnterPassword">Re-enter password*</Label>
                    <Input required id="reEnterPassword" name="reEnterPassword" type="password" />
                  </div>
                  <Button type="submit" className="w-full">
                    Reset password
                  </Button>
                </form>
              </CardContent>
            }
          </Card>
        </div>
      </div>
    </div>
  );
}
