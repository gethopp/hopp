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
  private _onPathRemoved: ((pathIds: number[]) => void) | null = null;

  constructor(color: string, drawingMode: TDrawingMode | null = null) {
    this.color = color;
    this._drawingMode = drawingMode;
  }

  /**
   * Set callback to be called when paths are removed
   */
  setOnPathRemoved(callback: (pathIds: number[]) => void): void {
    this._onPathRemoved = callback;
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
  handleDrawStart(point: TPClientPoint, pathId: number): void {
    this._inProgressPath = {
      id: pathId,
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
   * Calls the onPathRemoved callback if paths are removed
   */
  getAllPaths(): DrawingPath[] {
    // Clean up expired paths (lazy cleanup for non-permanent mode)
    const now = Date.now();
    const FIVE_SECONDS = 5000;
    const removedPathIds: number[] = [];

    if (this._drawingMode?.type === "Draw" && !this._drawingMode.settings.permanent) {
      this._completedPaths = this._completedPaths.filter((path) => {
        // Keep paths without timestamp (shouldn't happen, but safe) or paths less than 5 seconds old
        const shouldKeep = !path.completedAt || now - path.completedAt < FIVE_SECONDS;
        if (!shouldKeep) {
          removedPathIds.push(path.id);
        }
        return shouldKeep;
      });

      // Call callback if paths were removed
      if (removedPathIds.length > 0 && this._onPathRemoved) {
        this._onPathRemoved(removedPathIds);
      }
    }

    const paths = [...this._completedPaths];
    if (this._inProgressPath) {
      paths.push(this._inProgressPath);
    }
    return paths;
  }
}
