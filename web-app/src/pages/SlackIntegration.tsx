import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { CheckCircle } from "lucide-react";

/**
 * SlackIntegration page - shows success message after OAuth callback.
 * This is displayed at /integrations/slack/success after installing the Slack app.
 */
export function SlackIntegration() {
  const navigate = useNavigate();

  return (
    <div className="flex items-center justify-center min-h-[60vh]">
      <Card className="w-full max-w-md text-left">
        <CardHeader>
          <CardTitle className="text-2xl">
            <span className="mr-1.5">🏀</span> Hopp Installed Successfully!
          </CardTitle>
          <CardDescription>Your Slack workspace is now connected to Hopp.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <Alert className="bg-green-50 border-green-200">
            <CheckCircle className="h-4 w-4 text-green-600" />
            <AlertTitle className="text-green-800">Ready to use</AlertTitle>
            <AlertDescription className="text-green-700">
              You can now use <code className="bg-green-100 px-1 rounded">/hopp</code> in any Slack channel to start
              pairing sessions.
            </AlertDescription>
          </Alert>
          <div className="flex gap-2 justify-center">
            <Button onClick={() => navigate("/integrations")}>Manage Integrations</Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
