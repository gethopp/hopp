import { TPClientPoint, TDrawingMode } from "@/payloads";

export interface DrawingPath {
  id: number;
  points: TPClientPoint[];
  color: string;
  completedAt?: number; // Timestamp when path was completed (for non-permanent paths)
}

export class DrawParticipant {
  private _inProgressPath: DrawingPath | null = null;
  private _completedPaths: DrawingPath[] = [];
  public readonly color: string;
  private _drawingMode: TDrawingMode | null;
  private _pathIdCounter: number = 0;
  private static readonly MAX_PATH_ID = Number.MAX_SAFE_INTEGER;

  constructor(color: string, drawingMode: TDrawingMode | null = null) {
    this.color = color;
    this._drawingMode = drawingMode;
  }

  /**
   * Get the next unique path ID and handle wrap-over
   */
  private getNextPathId(): number {
    const id = this._pathIdCounter;
    this._pathIdCounter++;

    // Handle wrap-over: reset to 0 when reaching max
    if (this._pathIdCounter > DrawParticipant.MAX_PATH_ID) {
      this._pathIdCounter = 0;
    }

    return id;
  }

  /**
   * Get the current in-progress path
   */
  get inProgressPath(): DrawingPath | null {
    return this._inProgressPath;
  }

  /**
   * Get all completed paths
   */
  get completedPaths(): DrawingPath[] {
    return this._completedPaths;
  }

  /**
   * Get the drawing mode (only relevant for local participant)
   */
  get drawingMode(): TDrawingMode | null {
    return this._drawingMode;
  }

  /**
   * Set the drawing mode (only relevant for local participant)
   */
  setDrawingMode(mode: TDrawingMode): void {
    this._drawingMode = mode;

    if (mode.type === "Disabled") {
      this.clear();
    }
  }

  /**
   * Handle DrawStart event - begins a new path
   */
  handleDrawStart(point: TPClientPoint): void {
    this._inProgressPath = {
      id: this.getNextPathId(),
      points: [point],
      color: this.color,
    };
  }

  /**
   * Handle DrawAddPoint event - adds a point to the current path
   */
  handleDrawAddPoint(point: TPClientPoint): void {
    if (this._inProgressPath) {
      this._inProgressPath.points.push(point);
    }
  }

  /**
   * Handle DrawEnd event - completes the current path
   */
  handleDrawEnd(point: TPClientPoint): void {
    if (this._inProgressPath) {
      this._inProgressPath.points.push(point);

      // Add timestamp if in non-permanent mode (for automatic cleanup)
      if (this._drawingMode?.type === "Draw" && !this._drawingMode.settings.permanent) {
        this._inProgressPath.completedAt = Date.now();
      }

      this._completedPaths.push(this._inProgressPath);
      this._inProgressPath = null;
    }
  }

  /**
   * Clear all paths (both in-progress and completed)
   */
  clear(): void {
    this._inProgressPath = null;
    this._completedPaths = [];
  }

  /**
   * Get all paths that should be rendered (completed + in-progress)
   * Automatically cleans up expired paths in non-permanent mode
   */
  getAllPaths(): DrawingPath[] {
    // Clean up expired paths (lazy cleanup for non-permanent mode)
    const now = Date.now();
    const FIVE_SECONDS = 5000;

    if (this._drawingMode?.type === "Draw" && !this._drawingMode.settings.permanent) {
      this._completedPaths = this._completedPaths.filter((path) => {
        // Keep paths without timestamp (shouldn't happen, but safe) or paths less than 5 seconds old
        return !path.completedAt || now - path.completedAt < FIVE_SECONDS;
      });
    }

    const paths = [...this._completedPaths];
    if (this._inProgressPath) {
      paths.push(this._inProgressPath);
    }
    return paths;
  }
}
