export class CursorParticipant {
  public readonly participantId: string;
  public participantName: string;
  public x: number;
  public y: number;
  public lastActivity: number;
  public readonly color: string;

  constructor(participantId: string, participantName: string, color: string, x: number = -1000, y: number = -1000) {
    this.participantId = participantId;
    this.participantName = participantName;
    this.color = color;
    this.x = x;
    this.y = y;
    this.lastActivity = Date.now();
  }

  /**
   * Update cursor position and activity timestamp
   */
  updatePosition(x: number, y: number): void {
    this.x = x;
    this.y = y;
    this.lastActivity = Date.now();
  }

  /**
   * Update participant name (for unique name generation)
   */
  updateName(name: string): void {
    this.participantName = name;
  }

  /**
   * Hide cursor by moving it off-screen
   */
  hide(): void {
    this.x = -1000;
    this.y = -1000;
  }

  /**
   * Check if cursor should be hidden based on inactivity timeout
   */
  shouldHide(inactivityTimeout: number = 5000): boolean {
    return Date.now() - this.lastActivity > inactivityTimeout;
  }
}
