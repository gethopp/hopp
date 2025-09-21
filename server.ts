// server.ts

console.log("Starting WebSocket server on ws://localhost:1994");

Deno.serve({ port: 1994 }, (req) => {
  // To handle a WebSocket connection, we need to upgrade the HTTP request.
  // Deno.upgradeWebSocket handles the handshake and returns a WebSocket
  // object and the necessary response to complete the handshake.
  if (req.headers.get("upgrade") !== "websocket") {
    return new Response("Please use a WebSocket client.", { status: 400 });
  }

  const { socket, response } = Deno.upgradeWebSocket(req);

  let intervalId: number | undefined;

  // This event fires when the connection is established.
  socket.onopen = () => {
    console.log("Client connected");
    socket.send("connection received");

    // We set an interval to send a message to the client every second.
    intervalId = setInterval(() => {
      console.log("sending message");
      // Ensure the socket is still open before sending.
      if (socket.readyState === WebSocket.OPEN) {
        socket.send("sending message!");
      }
    }, 1000);
  };

  // This event fires when a message is received from the client.
  socket.onmessage = (event) => {
    console.log(`received: ${event.data}`);
    // As in the original script, we close the connection upon receiving a message.
    socket.close();
  };

  // This event fires when the connection is closed.
  socket.onclose = () => {
    console.log("Client disconnected");
    // It's crucial to clear the interval to prevent memory leaks.
    if (intervalId) {
      clearInterval(intervalId);
    }
  };

  // This event fires if an error occurs.
  socket.onerror = (err) => {
    console.error("WebSocket error:", err);
  };

  return response;
});
