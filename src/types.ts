export type Level3 = "LOW" | "MID" | "HIGH";
export type ParamType = "gravity" | "speed" | "friction";

export type MatchStatus = "WAITING" | "RUNNING" | "FINISHED";

export interface PlayerParams {
  gravity: Level3;
  speed: Level3;
  friction: Level3;
}

export interface PlayerState {
  playerId: string;
  displayName: string;
  connected: boolean;
  progress: number;
  params: PlayerParams;
}

export interface StageConfig {
  stageId: string;
  name: string;
  enabledParams: ParamType[];
}

export interface Match {
  matchId: string;
  stageId: string;
  status: MatchStatus;
  createdAt: string;
  seed: number;
  maxPlayers: number;
  players: PlayerState[];
}
