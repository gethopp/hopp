import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { toast } from "react-hot-toast";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import Logo from "@/assets/Hopp.png";
import { BACKEND_URLS } from "@/constants";

interface ForgotPasswordResponse {
  message: string;
}

export function ForgotPassword() {
  const [isFormSubmitted, setFormSubmitted] = useState(false);
  const [email, setEmail] = useState("");
  const [message, setMessage] = useState("");

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      const endpoint = "/api/forgot-password";
      const response = await fetch(`${BACKEND_URLS.BASE}${endpoint}`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          email: email,
        }),
      });

      const data = (await response.json()) as ForgotPasswordResponse;

      if (!response.ok) {
        throw new Error(data.message);
      }
      setMessage(data.message);
      setFormSubmitted(true);
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : "Authentication failed";
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
              <CardTitle className="text-xl">{isFormSubmitted ? "Link sent!" : "Forgot your password?"}</CardTitle>
              <CardDescription>
                {isFormSubmitted && message ? `${message}` : "Enter your email so we can send you password reset link"}
              </CardDescription>
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
                    <Label htmlFor="email">E-mail</Label>
                    <Input
                      required
                      id="email"
                      value={email}
                      type="email"
                      onChange={(e) => setEmail(e.target.value)}
                      placeholder="e.g. dwight@dundermifflin.com"
                    />
                  </div>
                  <Button type="submit" className="w-full">
                    Send Email
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
