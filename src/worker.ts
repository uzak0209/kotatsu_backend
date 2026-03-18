import { applyParamDelta } from "./domain";
import { Match, ParamType, PlayerState, StageConfig } from "./types";

export interface Env {
  GAME_KV?: KVNamespace;
}

const memory = new Map<string, string>();

const defaultStages: StageConfig[] = [
  {
    stageId: "stage-1",
    name: "Classic Holes",
    enabledParams: ["gravity", "speed", "friction"],
  },
  {
    stageId: "stage-2",
    name: "Ice Runner",
    enabledParams: ["speed", "friction"],
  },
  {
    stageId: "stage-3",
    name: "Heavy Sky",
    enabledParams: ["gravity", "speed", "friction"],
  },
];

async function kvGet<T>(env: Env, key: string): Promise<T | null> {
  const raw = env.GAME_KV
    ? await env.GAME_KV.get(key)
    : (memory.get(key) ?? null);
  return raw ? (JSON.parse(raw) as T) : null;
}

async function kvPut(env: Env, key: string, value: unknown): Promise<void> {
  const raw = JSON.stringify(value);
  if (env.GAME_KV) {
    await env.GAME_KV.put(key, raw);
    return;
  }
  memory.set(key, raw);
}

function json(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
    },
  });
}

function parseBody<T>(req: Request): Promise<T> {
  return req.json() as Promise<T>;
}

function newId(prefix: string): string {
  return `${prefix}_${crypto.randomUUID().slice(0, 8)}`;
}

function defaultParams() {
  return {
    gravity: "MID" as const,
    speed: "MID" as const,
    friction: "MID" as const,
  };
}

async function getMatch(env: Env, matchId: string): Promise<Match | null> {
  return kvGet<Match>(env, `match:${matchId}`);
}

async function saveMatch(env: Env, match: Match): Promise<void> {
  await kvPut(env, `match:${match.matchId}`, match);
}

function corsHeaders(req: Request): HeadersInit {
  const origin = req.headers.get("origin") ?? "*";
  return {
    "access-control-allow-origin": origin,
    "access-control-allow-methods": "GET,POST,OPTIONS",
    "access-control-allow-headers": "content-type,authorization",
  };
}

export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    if (req.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: corsHeaders(req) });
    }

    const url = new URL(req.url);
    const path = url.pathname;

    try {
      let res: Response;

      if (req.method === "GET" && path === "/health") {
        res = json({ ok: true, now: new Date().toISOString() });
      } else if (req.method === "GET" && path === "/v1/stages") {
        res = json({ stages: defaultStages });
      } else if (req.method === "POST" && path === "/v1/matches") {
        const body = await parseBody<{
          stageId?: string;
          maxPlayers?: number;
          seed?: number;
        }>(req);

        const stageId = body.stageId ?? defaultStages[0].stageId;
        const stage = defaultStages.find((s) => s.stageId === stageId);
        if (!stage) {
          return json({ error: "invalid_stage_id" }, 400);
        }

        const maxPlayers = Math.max(2, Math.min(8, body.maxPlayers ?? 4));
        const match: Match = {
          matchId: newId("m"),
          stageId,
          status: "WAITING",
          createdAt: new Date().toISOString(),
          seed: body.seed ?? Math.floor(Math.random() * 1_000_000_000),
          maxPlayers,
          players: [],
        };
        await saveMatch(env, match);
        res = json({ match });
      } else if (
        req.method === "POST" &&
        /^\/v1\/matches\/[^/]+\/join$/.test(path)
      ) {
        const [, , , matchId] = path.split("/");
        const body = await parseBody<{ displayName?: string }>(req);
        const match = await getMatch(env, matchId);
        if (!match) {
          return json({ error: "match_not_found" }, 404);
        }
        if (match.status !== "WAITING") {
          return json({ error: "match_not_joinable" }, 409);
        }
        if (match.players.length >= match.maxPlayers) {
          return json({ error: "match_full" }, 409);
        }

        const player: PlayerState = {
          playerId: newId("p"),
          displayName: body.displayName?.trim() || `player-${match.players.length + 1}`,
          connected: true,
          progress: 0,
          params: defaultParams(),
        };

        match.players.push(player);
        await saveMatch(env, match);
        res = json({ matchId, player });
      } else if (
        req.method === "POST" &&
        /^\/v1\/matches\/[^/]+\/start$/.test(path)
      ) {
        const [, , , matchId] = path.split("/");
        const match = await getMatch(env, matchId);
        if (!match) {
          return json({ error: "match_not_found" }, 404);
        }
        if (match.status !== "WAITING") {
          return json({ error: "invalid_match_state" }, 409);
        }
        if (match.players.length < 1) {
          return json({ error: "not_enough_players" }, 409);
        }

        match.status = "RUNNING";
        await saveMatch(env, match);
        res = json({ match });
      } else if (req.method === "GET" && /^\/v1\/matches\/[^/]+$/.test(path)) {
        const [, , , matchId] = path.split("/");
        const match = await getMatch(env, matchId);
        if (!match) {
          return json({ error: "match_not_found" }, 404);
        }
        res = json({ match });
      } else if (
        req.method === "POST" &&
        /^\/v1\/matches\/[^/]+\/params\/apply$/.test(path)
      ) {
        const [, , , matchId] = path.split("/");
        const body = await parseBody<{
          playerId?: string;
          param?: ParamType;
          delta?: -1 | 1;
        }>(req);

        const match = await getMatch(env, matchId);
        if (!match) {
          return json({ error: "match_not_found" }, 404);
        }
        if (match.status !== "RUNNING") {
          return json({ error: "match_not_running" }, 409);
        }
        if (!body.playerId || !body.param || !body.delta) {
          return json({ error: "invalid_payload" }, 400);
        }

        const stage = defaultStages.find((s) => s.stageId === match.stageId);
        if (!stage || !stage.enabledParams.includes(body.param)) {
          return json({ error: "param_not_enabled_for_stage" }, 409);
        }

        const player = match.players.find((p) => p.playerId === body.playerId);
        if (!player) {
          return json({ error: "player_not_found" }, 404);
        }

        player.params = applyParamDelta(player.params, body.param, body.delta);
        await saveMatch(env, match);

        res = json({
          ok: true,
          playerId: player.playerId,
          param: body.param,
          newValue: player.params[body.param],
        });
      } else {
        res = json({ error: "not_found" }, 404);
      }

      const headers = new Headers(res.headers);
      Object.entries(corsHeaders(req)).forEach(([k, v]) => headers.set(k, String(v)));
      return new Response(await res.text(), { status: res.status, headers });
    } catch (error) {
      const message = error instanceof Error ? error.message : "unknown_error";
      const res = json({ error: "internal_error", message }, 500);
      const headers = new Headers(res.headers);
      Object.entries(corsHeaders(req)).forEach(([k, v]) => headers.set(k, String(v)));
      return new Response(await res.text(), { status: res.status, headers });
    }
  },
};
