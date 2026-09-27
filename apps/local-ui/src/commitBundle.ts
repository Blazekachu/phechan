/** Downloadable commit bundle for deferred inscription reveal (mirrors runes-etch UX). */

import type { InscribePlan } from "./api";
import {
  snapshotPlan,
  upsertInterruptedCommit,
  type InterruptedCommit,
} from "./interrupted";

export const BUNDLE_TYPE = "phechan-inscription-commit" as const;
export const BUNDLE_VERSION = 1 as const;
export const BUNDLE_MAX_BYTES = 2 * 1024 * 1024;

export type CommitBundle = {
  version: typeof BUNDLE_VERSION;
  type: typeof BUNDLE_TYPE;
  createdAt: string;
  network: string;
  commitTxid: string;
  commitAddress: string;
  /** Commit output value when known (from prepare / fund). */
  commitSats?: number;
  /** Snapshot needed to rebuild the same reveal later. */
  plan: InscribePlan;
};

export function createCommitBundle(params: {
  commitTxid: string;
  commitAddress: string;
  network: string;
  plan: InscribePlan;
  commitSats?: number;
}): CommitBundle {
  return {
    version: BUNDLE_VERSION,
    type: BUNDLE_TYPE,
    createdAt: new Date().toISOString(),
    network: params.network,
    commitTxid: params.commitTxid,
    commitAddress: params.commitAddress,
    ...(params.commitSats != null && params.commitSats > 0
      ? { commitSats: params.commitSats }
      : {}),
    plan: snapshotPlan(params.plan),
  };
}

export function bundleFilename(bundle: CommitBundle): string {
  const slug =
    bundle.plan.fileName?.replace(/[^\w.-]+/g, "_").slice(0, 40) ||
    bundle.plan.parentId?.slice(0, 12) ||
    bundle.plan.mode ||
    "inscription";
  return `${slug}_commit_${bundle.commitTxid.slice(0, 8)}.phechan.json`;
}

export function downloadCommitBundle(bundle: CommitBundle): void {
  const json = JSON.stringify(bundle, null, 2);
  const blob = new Blob([json], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = bundleFilename(bundle);
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

export function interruptedFromBundle(bundle: CommitBundle): InterruptedCommit {
  return {
    commitTxid: bundle.commitTxid,
    commitAddress: bundle.commitAddress,
    network: bundle.network,
    savedAt: Date.now(),
    plan: snapshotPlan({
      ...bundle.plan,
      commitTxid: bundle.commitTxid,
      ...(bundle.commitSats != null
        ? { commitValue: bundle.commitSats }
        : {}),
    }),
  };
}

export function parseCommitBundle(text: string): CommitBundle {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    throw new Error("Invalid JSON — expected a phechan commit bundle.");
  }
  if (!raw || typeof raw !== "object") {
    throw new Error("Bundle must be a JSON object.");
  }
  const o = raw as Record<string, unknown>;
  if (o.version !== BUNDLE_VERSION || o.type !== BUNDLE_TYPE) {
    throw new Error(
      `Unsupported bundle (need version ${BUNDLE_VERSION}, type ${BUNDLE_TYPE}).`
    );
  }
  const commitTxid = String(o.commitTxid || "");
  const commitAddress = String(o.commitAddress || "");
  const network = String(o.network || "");
  if (!/^[0-9a-fA-F]{64}$/.test(commitTxid)) {
    throw new Error("Bundle missing valid commitTxid.");
  }
  if (!commitAddress) {
    throw new Error("Bundle missing commitAddress.");
  }
  if (!network) {
    throw new Error("Bundle missing network.");
  }
  if (!o.plan || typeof o.plan !== "object") {
    throw new Error("Bundle missing plan snapshot.");
  }
  const plan = snapshotPlan(o.plan as InscribePlan);
  const commitSats =
    typeof o.commitSats === "number" && o.commitSats > 0
      ? o.commitSats
      : undefined;
  return {
    version: BUNDLE_VERSION,
    type: BUNDLE_TYPE,
    createdAt: String(o.createdAt || new Date().toISOString()),
    network,
    commitTxid,
    commitAddress,
    ...(commitSats != null ? { commitSats } : {}),
    plan,
  };
}

/** Parse upload, upsert into interrupted list, return the interrupted row. */
export function importCommitBundle(text: string): InterruptedCommit {
  const bundle = parseCommitBundle(text);
  const item = interruptedFromBundle(bundle);
  upsertInterruptedCommit(item);
  return item;
}

export function effectiveRevealFeeRate(plan: Pick<InscribePlan, "revealFeeRate" | "feeRate">): number {
  const r = plan.revealFeeRate ?? plan.feeRate;
  return r != null && Number(r) > 0 ? Number(r) : 1;
}

export function effectiveCommitFeeRate(plan: Pick<InscribePlan, "commitFeeRate" | "feeRate">): number {
  const r = plan.commitFeeRate ?? plan.feeRate;
  return r != null && Number(r) > 0 ? Number(r) : 1;
}
