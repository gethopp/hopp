export type Status = "idle" | "connecting" | "open" | "closing" | "closed" | "error";

class WSClientService {
  private socket: WebSocket | null = null;
  private url: string | null = null;
  private status: Status = "idle";
  private lastClose: { code?: number; reason?: string } | null = null;
  private lastError: string | null = null;
  private worker: Worker | null = null;

  private cleanupHandlers() {
    if (!this.socket) return;
    this.socket.onopen = null;
    this.socket.onclose = null;
    this.socket.onmessage = null;
    this.socket.onerror = null;
  }

  connect(url: string, onMessage: (data: ArrayBuffer, receivedAtMs: number) => void) {
    // If already connected to same URL, ignore
    if (this.url === url && (this.status === "open" || this.status === "connecting")) return;

    this.url = url;
    this.lastClose = null;
    this.lastError = null;
    this.status = "connecting";

    // Prefer worker path
    try {
      if (this.worker) {
        try { this.worker.terminate(); } catch {}
        this.worker = null;
      }
      const worker = new Worker(new URL("@/workers/wsVideo.worker.ts", import.meta.url), { type: "module" });
      worker.onmessage = (ev: MessageEvent) => {
        const msg = ev.data as any;
        if (!msg || typeof msg !== "object") return;
        if (msg.type === "STATUS") {
          this.status = msg.status;
          if (msg.status === "closed") this.socket = null;
        } else if (msg.type === "DATA") {
          onMessage(msg.data as ArrayBuffer, msg.receivedAtMs as number);
        }
      };
      this.worker = worker;
      worker.postMessage({ type: "INIT", url });
      return;
    } catch (_e) {
      // Fallback to main-thread WebSocket
    }

    // Fallback
    try {
      if (this.socket) {
        try { this.socket.close(1000, "reconnect"); } catch {}
        this.cleanupHandlers();
        this.socket = null;
      }
      const ws = new WebSocket(url);
      ws.binaryType = "arraybuffer";
      ws.onopen = () => { this.status = "open"; };
      ws.onclose = (e) => {
        this.lastClose = { code: e.code, reason: e.reason };
        this.status = "closed";
        this.cleanupHandlers();
        this.socket = null;
      };
      ws.onerror = () => { this.lastError = "WebSocket error"; this.status = "error"; };
      ws.onmessage = (event) => { onMessage(event.data, Date.now()); };
      this.socket = ws;
    } catch (e) {
      this.lastError = e instanceof Error ? e.message : "Unknown error";
      this.status = "error";
      this.cleanupHandlers();
      this.socket = null;
    }
  }

  disconnect() {
    if (this.worker) {
      try { this.worker.postMessage({ type: "CLOSE" }); } catch {}
      try { this.worker.terminate(); } catch {}
      this.worker = null;
    }
    if (!this.socket) {
      this.status = "closed";
      return;
    }
    if (this.socket.readyState === WebSocket.OPEN || this.socket.readyState === WebSocket.CONNECTING) {
      this.status = "closing";
      try { this.socket.close(1000, "client_close"); } catch {}
    } else {
      this.status = "closed";
    }
  }

  getStatus() {
    return this.status;
  }

  getLastClose() {
    return this.lastClose;
  }

  getLastError() {
    return this.lastError;
  }

  send(data: ArrayBuffer) {
    if (this.status !== "open") {
      console.warn("WSClient: Cannot send message, connection not open. Status:", this.status);
      return false;
    }

    try {
      if (this.worker) {
        // Send via worker - transfer the buffer to avoid copying
        this.worker.postMessage({ type: "SEND", data }, [data]);
        return true;
      } else if (this.socket && this.socket.readyState === WebSocket.OPEN) {
        // Send via direct WebSocket
        this.socket.send(data);
        return true;
      }
    } catch (e) {
      console.error("WSClient: Failed to send message:", e);
      return false;
    }

    return false;
  }
}

export const wsClient = new WSClientService();