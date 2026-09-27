/**
 * Interrupted / deferred commit+reveal jobs.
 * Also backing store for Profile + uploaded `*.phechan.json` commit bundles.
 */

import type { InscribePlan } from "./api";

const KEY = "phechan.interruptedCommits";

export type InterruptedCommit = {
  commitTxid: string;
  commitAddress: string;
  network: string;
  savedAt: number;
  /** Snapshot of fields needed to rebuild the same reveal */
  plan: InscribePlan;
};

function readAll(): InterruptedCommit[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw) as InterruptedCommit[];
    return Array.isArray(arr) ? arr.filter((x) => x?.commitTxid) : [];
  } catch {
    return [];
  }
}

function writeAll(items: InterruptedCommit[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(items.slice(0, 40)));
  } catch {
    /* ignore */
  }
}

export function listInterruptedCommits(): InterruptedCommit[] {
  return readAll().sort((a, b) => b.savedAt - a.savedAt);
}

export function upsertInterruptedCommit(item: InterruptedCommit) {
  const rest = readAll().filter((x) => x.commitTxid !== item.commitTxid);
  writeAll([{ ...item, savedAt: Date.now() }, ...rest]);
}

export function removeInterruptedCommit(commitTxid: string) {
  writeAll(readAll().filter((x) => x.commitTxid !== commitTxid));
}

/** Strip non-serializable / runtime-only fields from plan for storage. */
export function snapshotPlan(plan: InscribePlan): InscribePlan {
  return JSON.parse(JSON.stringify(plan)) as InscribePlan;
}
