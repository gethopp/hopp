// In worker context, self is defined. Avoid redeclaration for TS.
// Minimal WebSocket client in a Worker.
// Message protocol from main:
// - { type: "INIT", url: string }
// - { type: "CLOSE" }
// Messages to main:
// - { type: "STATUS", status: "connecting" | "open" | "closed" | "error" }
// - { type: "DATA", data: ArrayBuffer, receivedAtMs: number }

let socket: WebSocket | null = null;

function cleanup() {
  if (!socket) return;
  socket.onopen = null;
  socket.onclose = null;
  socket.onmessage = null;
  socket.onerror = null;
  socket = null;
}

function postStatus(status: "connecting" | "open" | "closed" | "error") {
  (self as any).postMessage({ type: "STATUS", status });
}

function connect(url: string) {
  try {
    if (socket) {
      try { socket.close(1000, "reconnect"); } catch {}
      cleanup();
    }
    postStatus("connecting");
    socket = new WebSocket(url);
    socket.binaryType = "arraybuffer";
    socket.onopen = () => {
      postStatus("open");
    };
    socket.onclose = () => {
      postStatus("closed");
      cleanup();
    };
    socket.onerror = () => {
      postStatus("error");
    };
    socket.onmessage = (ev: MessageEvent) => {
      const data = ev.data as ArrayBuffer;
      const receivedAtMs = Date.now();
      // Transfer the buffer to main thread to avoid copy
      (self as any).postMessage({ type: "DATA", data, receivedAtMs }, [data as any as Transferable]);
    };
  } catch (_e) {
    postStatus("error");
    cleanup();
  }
}

(self as any).onmessage = (ev: MessageEvent) => {
  const msg = ev.data;
  if (!msg || typeof msg !== "object") return;
  switch (msg.type) {
    case "INIT":
      connect(msg.url);
      break;
    case "CLOSE":
      if (socket) {
        try { socket.close(1000, "client_close"); } catch {}
      }
      cleanup();
      postStatus("closed");
      break;
  }
};


