import { URLS } from "@/constants";
import useStore from "../store/store";
import { WebSocket } from "partysocket";
import { tauriUtils } from "@/windows/window-utils";

class SocketService {
  private socket: WebSocket | null = null;
  private baseUrl = `wss://${URLS.API_BASE_URL}/api/auth/websocket`;
  private pingTimeout: NodeJS.Timeout | null = null;
  private messageHandlers = new Map<string, (data: any) => void>();
  private currentToken: string | null = null;

  constructor() {
    useStore.subscribe((state) => {
      const token = state.authToken;
      if (token && token !== this.currentToken) {
        console.log("Token changed, updating socket connection:", token);
        this.currentToken = token;
        this.connect(token);
      } else if (!token && this.currentToken) {
        console.log("Token removed, closing socket connection.");
        this.currentToken = null;
        this.closeConnection();
      }
    });
  }

  async init() {
    const token = await tauriUtils.getStoredToken();
    if (token && token !== this.currentToken) {
      console.log("Initializing socket with stored token:", token);
      this.currentToken = token;
      this.connect(token);
    }
  }

  public updateToken(token: string) {
    if (token && token !== this.currentToken) {
      console.log("Closing socket connection and updating socket connection with new token:", token);
      this.closeConnection();
      this.currentToken = token;
      this.connect(token);
    }
  }

  private connect(token: string) {
    this.closeConnection();

    console.log("Connecting 📶:", token);

    try {
      this.socket = new WebSocket(`${this.baseUrl}?token=${token}`, [], {
        minReconnectionDelay: 200,
        maxReconnectionDelay: 1000,
      });
      this.setupEventListeners();
    } catch (error) {
      console.error("Failed to create WebSocket:", error);
      useStore.getState().setSocketConnected(false);
    }
  }

  private closeConnection() {
    if (this.socket) {
      console.log("Closing existing socket connection.");
      this.socket.close(1000, "Closing connection");
      this.socket = null;
      this.stopHeartbeat();
      useStore.getState().setSocketConnected(false);
    }
  }

  private setupEventListeners() {
    if (!this.socket) return;

    this.socket.addEventListener("open", () => {
      console.log("Socket connected");
      useStore.getState().setSocketConnected(true);
      this.startHeartbeat();
    });

    this.socket.addEventListener("close", (event: CloseEvent) => {
      console.log("Socket disconnected:", event.code, event.reason);
      useStore.getState().setSocketConnected(false);
      this.stopHeartbeat();
    });

    this.socket.addEventListener("error", (event: Event) => {
      console.error("Socket connection error:", JSON.stringify(event));
      useStore.getState().setSocketConnected(false);
    });

    this.socket.addEventListener("message", (event: MessageEvent) => {
      try {
        const parsedData = JSON.parse(event.data);
        this.emit(parsedData);
      } catch (error) {
        console.error("Error parsing message:", error);
      }
    });
  }

  private startHeartbeat() {
    // Clear any existing heartbeat
    this.stopHeartbeat();

    // Set up heartbeat check
    this.pingTimeout = setInterval(() => {
      if (this.socket?.readyState === WebSocket.OPEN) {
        // Send a simple ping message
        this.send({
          type: "ping",
          payload: {
            message: "ping",
          },
        });
      }
    }, 30_000); // Send ping every 30 seconds
  }

  private stopHeartbeat() {
    if (this.pingTimeout) {
      clearInterval(this.pingTimeout);
      this.pingTimeout = null;
    }
  }

  // Method to send messages
  public send(data: any) {
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
      console.warn("Socket not connected, cannot send message");
      return;
    }
    this.socket.send(JSON.stringify(data));
  }

  private emit(data: any) {
    this.messageHandlers.forEach((handler) => handler(data));
  }

  // Method to add custom message listeners
  // Also removes any existing listener with the same id
  public on(id: string, callback: (data: any) => void) {
    // Remove any existing listener with the same id
    this.messageHandlers.delete(id);
    this.messageHandlers.set(id, callback);
  }

  // Method to remove message listener
  public removeHandler(id: string) {
    this.messageHandlers.delete(id);
  }
}

// Create a singleton instance
export const socketService = new SocketService();
