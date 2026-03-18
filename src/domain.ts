import { Level3, ParamType, PlayerParams } from "./types";

const ORDER: Level3[] = ["LOW", "MID", "HIGH"];

export function shiftLevel(current: Level3, delta: -1 | 1): Level3 {
  const idx = ORDER.indexOf(current);
  const next = Math.max(0, Math.min(ORDER.length - 1, idx + delta));
  return ORDER[next];
}

export function applyParamDelta(
  params: PlayerParams,
  param: ParamType,
  delta: -1 | 1,
): PlayerParams {
  return {
    ...params,
    [param]: shiftLevel(params[param], delta),
  };
}
