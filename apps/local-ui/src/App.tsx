import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type CliResponse, type InscribeMode, type InscribePlan } from "./api";
import {
  listInterruptedCommits,
  removeInterruptedCommit,
  snapshotPlan,
  upsertInterruptedCommit,
  type InterruptedCommit,
} from "./interrupted";
import {
  createCommitBundle,
  downloadCommitBundle,
  importCommitBundle,
} from "./commitBundle";
import {
  connectWallet,
  detectWallet,
  explorerTxUrl,
  getWalletUtxos,
  paymentAddressType,
  pickFundingUtxo,
  signPsbtWithWallet,
} from "./wallet";
import {
  estimateVanity,
  MAX_VANITY_TOTAL,
  sanitizeVanityHex,
} from "./vanity";

type VerifyUi = "idle" | "loading" | "ok" | "error";
type FlowPhase =
  | "idle"
  | "preparing"
  | "funding"
  | "review"
  | "grinding"
  | "committed"
  | "done"
  | "error";
type AppPage = "inscribe" | "profile" | "studio";
/** Fast = commit+reveal in one session. Control = fund now, reveal later (bundle). */
type InscribePace = "fast" | "control";

const PACE_KEY = "phechan.inscribePace";

function readStoredPace(): InscribePace {
  try {
    const v = localStorage.getItem(PACE_KEY);
    if (v === "control" || v === "fast") return v;
  } catch {
    /* ignore */
  }
  return "fast";
}

type SignReview = {
  kind: "funding" | "reveal";
  headline: string;
  steps: string[];
  warnings: string[];
  inputs: string[];
  outputs: string[];
  meta: string[];
  buttonLabel: string;
};
function sleep(ms: number) {
  return new Promise((r) => setTimeout(r, ms));
}

function isWalletCancel(msg: string) {
  // Do not treat wallet "txn error" / validation failures as cancel (those need a hard stop).
  if (/txn error|transaction error|unknown output|unspendable|invalid/i.test(msg)) {
    return false;
  }
  return /cancel|reject|denied|4001/i.test(msg);
}

function isTransientError(msg: string) {
  return /timeout|network|fetch|ECONN|temporar|503|502|429|esplora/i.test(msg);
}

/** CLI fields may arrive as string | string[] when a key was printed more than once. */
function cliScalar(v: unknown): string {
  if (Array.isArray(v)) return String(v[0] ?? "");
  if (v == null) return "";
  return String(v);
}

function cliNumber(v: unknown): number {
  const n = Number(cliScalar(v));
  return Number.isFinite(n) ? n : 0;
}

/** Parse sat/vB from UI fields — supports fractional rates (0.1, 0.69, …) and any rate > 0. */
function parseFeeRateInput(raw: unknown, fallback = 1): number {
  const n = typeof raw === "number" ? raw : Number(String(raw ?? "").trim());
  return Number.isFinite(n) && n > 0 ? n : fallback;
}

/** Commit vbytes à la inscribe.dev (Wizards of Ord). Native + nested segwit. */
function estimateCommitVbytesInscribe(opts: {
  carrier: boolean;
  paymentType: "p2wpkh" | "p2sh-p2wpkh" | "p2tr" | "unknown";
}): number {
  const base = 10.5 + 43; // base + commit P2TR out
  const change =
    opts.paymentType === "p2tr"
      ? 43
      : opts.paymentType === "p2sh-p2wpkh"
        ? 32
        : 31; // native P2WPKH (default for unknown)
  const payIn =
    opts.paymentType === "p2tr"
      ? 57.5
      : opts.paymentType === "p2sh-p2wpkh"
        ? 91
        : 67.75; // native P2WPKH
  return Math.ceil(base + change + payIn + (opts.carrier ? 57.5 : 0));
}

function formatResult(data: CliResponse): string {
  if (data.error) return `error: ${data.error}`;
  const lines: string[] = [];
  if (typeof data.ok === "boolean") lines.push(`ok: ${data.ok}`);
  for (const [k, v] of Object.entries(data)) {
    if (k === "ok" || k === "error" || k === "raw" || k === "fields") continue;
    if (v === undefined) continue;
    if (Array.isArray(v)) for (const item of v) lines.push(`${k}: ${item}`);
    else if (typeof v === "object") lines.push(`${k}: ${JSON.stringify(v)}`);
    else lines.push(`${k}: ${v}`);
  }
  if (data.raw && Array.isArray(data.raw)) {
    lines.push("---");
    lines.push(...data.raw);
  }
  return lines.join("\n");
}

function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const s = String(reader.result || "");
      const i = s.indexOf(",");
      resolve(i >= 0 ? s.slice(i + 1) : s);
    };
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

export default function App() {
  const [health, setHealth] = useState("checking…");
  const [busy, setBusy] = useState(false);
  const revealAbortRef = useRef<AbortController | null>(null);
  const [revealingCommitTxid, setRevealingCommitTxid] = useState<string | null>(null);
  const [out, setOut] = useState("");
  const [ok, setOk] = useState<boolean | null>(null);

  const [walletReady, setWalletReady] = useState(detectWallet().available);
  const [address, setAddress] = useState("");
  const [paymentAddress, setPaymentAddress] = useState("");
  const [paymentPublicKey, setPaymentPublicKey] = useState("");
  const [ordinalsPublicKey, setOrdinalsPublicKey] = useState("");
  const [network, setNetwork] = useState("signet");
  const [walletNote, setWalletNote] = useState(
    detectWallet().available
      ? "Wallet detected — connect to load address & network"
      : "No wallet detected"
  );

  const payType = paymentAddressType(paymentAddress || address);

  const [mode, setMode] = useState<InscribeMode>("text");
  const [body, setBody] = useState("Hello from Phechan");
  const [delegateId, setDelegateId] = useState("");
  const [delegateVerify, setDelegateVerify] = useState<VerifyUi>("idle");
  const [delegateMsg, setDelegateMsg] = useState("");
  const [uploadName, setUploadName] = useState("");
  const [uploadType, setUploadType] = useState("");
  const [uploadB64, setUploadB64] = useState("");

  const [showOptional, setShowOptional] = useState(false);
  const [title, setTitle] = useState("");
  const [metadata, setMetadata] = useState("");
  const [metaprotocol, setMetaprotocol] = useState("");
  const [compressBr, setCompressBr] = useState(false);

  const [showAdvanced, setShowAdvanced] = useState(false);
  const [parentId, setParentId] = useState("");
  const [parentVerify, setParentVerify] = useState<VerifyUi>("idle");
  const [parentMsg, setParentMsg] = useState("");
  const [parentOutpoint, setParentOutpoint] = useState("");
  const [parentValue, setParentValue] = useState<number | null>(null);
  const [parentAddress, setParentAddress] = useState("");
  const [satTarget, setSatTarget] = useState("");
  const [satVerify, setSatVerify] = useState<VerifyUi>("idle");
  const [satMsg, setSatMsg] = useState("");
  const [satReinscribe, setSatReinscribe] = useState(false);
  const [satResolvedOutpoint, setSatResolvedOutpoint] = useState("");
  const [satResolvedValue, setSatResolvedValue] = useState<number | null>(null);

  const [showTx, setShowTx] = useState(false);
  const [opReturn, setOpReturn] = useState("");
  const [vanityPrefix, setVanityPrefix] = useState("");
  const [vanitySuffix, setVanitySuffix] = useState("");
  const [commitVanityPrefix, setCommitVanityPrefix] = useState("");
  const [commitVanitySuffix, setCommitVanitySuffix] = useState("");
  const [commitFeeRate, setCommitFeeRate] = useState("1");
  const [revealFeeRate, setRevealFeeRate] = useState("1");
  const [inscribePace, setInscribePace] = useState<InscribePace>(() => readStoredPace());
  const [postage, setPostage] = useState("546");
  const [networkCost, setNetworkCost] = useState<{
    status: "idle" | "loading" | "ok" | "error";
    /** Miner fees only: reveal + estimated commit */
    networkFeeSats?: number;
    revealFeeSats?: number;
    commitFeeSats?: number;
    /** Postage — returned with inscription, not a miner fee */
    postageSats?: number;
    /** Amount to send to commit address = postage + reveal fee */
    fundCommitSats?: number;
    commitAddress?: string;
    detail?: string;
  }>({ status: "idle" });
  const [fundingUtxos, setFundingUtxos] = useState<
    {
      txid: string;
      vout: number;
      value: number;
      scriptPubKey?: string;
      address?: string;
      confirmations?: number | null;
    }[]
  >([]);
  const [selectedUtxoKey, setSelectedUtxoKey] = useState("");
  const [brotliStats, setBrotliStats] = useState<{
    rawBytes: number;
    compressedBytes: number;
    savedBytes: number;
    savedPct: number;
    savedVbytes: number;
    savedFeeSats: number;
  } | null>(null);

  const [showPreview, setShowPreview] = useState(false);
  const [flowPhase, setFlowPhase] = useState<FlowPhase>("idle");
  const [grindNote, setGrindNote] = useState("");
  const [resultTx, setResultTx] = useState<{
    commitTxid?: string;
    revealTxid?: string;
    inscriptionId?: string;
  } | null>(null);
  /** Deferred reveal after commit broadcast (bundle downloaded; reveal on demand). */
  const [interrupted, setInterrupted] = useState<InterruptedCommit[]>(() =>
    listInterruptedCommits()
  );
  const [pendingReveal, setPendingReveal] = useState<InterruptedCommit | null>(null);
  const bundleFileRef = useRef<HTMLInputElement | null>(null);
  const [page, setPage] = useState<AppPage>("inscribe");
  const [signReview, setSignReview] = useState<SignReview | null>(null);
  const signReviewResolver = useRef<((go: boolean) => void) | null>(null);

  const refreshInterrupted = useCallback(() => {
    setInterrupted(listInterruptedCommits());
  }, []);

  const setPace = useCallback((pace: InscribePace) => {
    setInscribePace(pace);
    try {
      localStorage.setItem(PACE_KEY, pace);
    } catch {
      /* ignore */
    }
    // Fast uses one rate — keep both fields aligned when switching back.
    if (pace === "fast") {
      setCommitFeeRate(revealFeeRate);
    }
  }, [revealFeeRate]);

  const isControlPace = inscribePace === "control";

  const isMainnetNet =
    network.toLowerCase() === "mainnet" || network.toLowerCase() === "bitcoin";
  const isRegtestConnected =
    Boolean(address) && network.toLowerCase() === "regtest";
  /** Reinscription: postage must equal the target UTXO value (not editable). */
  const postageLocked =
    satVerify === "ok" && satReinscribe && satResolvedValue != null && satResolvedValue > 0;

  const requestSignReview = useCallback((review: SignReview) => {
    setSignReview(review);
    setFlowPhase("review");
    setBusy(false);
    return new Promise<boolean>((resolve) => {
      signReviewResolver.current = resolve;
    });
  }, []);

  const approveSignReview = useCallback(() => {
    setBusy(true);
    const kind = signReview?.kind;
    setFlowPhase(kind === "reveal" ? "grinding" : "funding");
    const resolve = signReviewResolver.current;
    signReviewResolver.current = null;
    resolve?.(true);
  }, [signReview]);

  const cancelSignReview = useCallback(() => {
    const resolve = signReviewResolver.current;
    signReviewResolver.current = null;
    setSignReview(null);
    setFlowPhase("idle");
    setBusy(false);
    setOk(null);
    setOut("Cancelled — nothing broadcast.");
    resolve?.(false);
  }, []);

  const vanityDiff = useMemo(
    () => estimateVanity(vanityPrefix, vanitySuffix),
    [vanityPrefix, vanitySuffix]
  );
  const commitVanityDiff = useMemo(
    () => estimateVanity(commitVanityPrefix, commitVanitySuffix),
    [commitVanityPrefix, commitVanitySuffix]
  );

  /** Effective MIME for Brotli eligibility (text mode defaults to text/plain). */
  const effectiveContentType = useMemo(() => {
    if (mode === "upload") return uploadType || "application/octet-stream";
    if (mode === "delegate") return "";
    return "text/plain;charset=utf-8";
  }, [mode, uploadType]);

  const brotliEligible = useMemo(() => {
    const ct = effectiveContentType.split(";")[0].trim().toLowerCase();
    if (!ct) return false;
    return (
      ct.startsWith("text/") ||
      ct === "application/json" ||
      ct === "application/javascript" ||
      ct === "application/xml" ||
      ct === "image/svg+xml"
    );
  }, [effectiveContentType]);

  useEffect(() => {
    api
      .health()
      .then((h) => {
        setHealth(h.ok ? `API ok @ ${h.bind}` : "API unhealthy");
      })
      .catch(() => setHealth("API unreachable"));
    setWalletReady(detectWallet().available);
    try {
      localStorage.removeItem("phechan.pendingCommitTxid");
      localStorage.removeItem("phechan.pendingCommitAddress");
    } catch {
      /* ignore */
    }
    refreshInterrupted();
  }, [refreshInterrupted]);

  useEffect(() => {
    if (postageLocked && satResolvedValue != null) {
      setPostage(String(satResolvedValue));
    }
  }, [postageLocked, satResolvedValue]);

  useEffect(() => {
    if (page === "studio" && !isRegtestConnected) setPage("inscribe");
  }, [page, isRegtestConnected]);

  useEffect(() => {
    if (!brotliEligible) {
      setBrotliStats(null);
      return;
    }
    let cancelled = false;
    const t = window.setTimeout(() => {
      void (async () => {
        try {
          const r = await api.brotliPreview({
            body: mode === "text" ? body : undefined,
            contentBase64: mode === "upload" ? uploadB64 || undefined : undefined,
            feeRate: parseFeeRateInput(revealFeeRate),
            revealFeeRate: parseFeeRateInput(revealFeeRate),
          });
          if (cancelled || r.error) return;
          const savedBytes = Number(r.savedBytes) || 0;
          setBrotliStats({
            rawBytes: Number(r.rawBytes) || 0,
            compressedBytes: Number(r.compressedBytes) || 0,
            savedBytes,
            savedPct: Number(r.savedPct) || 0,
            savedVbytes: Number(r.savedVbytes) || 0,
            savedFeeSats: Number(r.savedFeeSats) || 0,
          });
          // Don't leave compression on when it would enlarge the body
          if (savedBytes <= 0) setCompressBr(false);
        } catch {
          if (!cancelled) setBrotliStats(null);
        }
      })();
    }, 300);
    return () => {
      cancelled = true;
      window.clearTimeout(t);
    };
  }, [brotliEligible, mode, body, uploadB64, revealFeeRate]);

  const plan: InscribePlan = useMemo(() => {
    const satForEngine = satResolvedOutpoint || satTarget || undefined;
    const sameSat =
      Boolean(parentOutpoint) &&
      ((satResolvedOutpoint &&
        satResolvedOutpoint.toLowerCase() === parentOutpoint.toLowerCase()) ||
        (satTarget.trim() &&
          parentId.trim() &&
          satTarget.trim().toLowerCase() === parentId.trim().toLowerCase()));
    return {
      mode,
      network,
      body: mode === "text" ? body : undefined,
      contentBase64: mode === "upload" ? uploadB64 || undefined : undefined,
      contentType: mode === "upload" ? uploadType || "application/octet-stream" : undefined,
      fileName: mode === "upload" ? uploadName || undefined : undefined,
      delegateId: mode === "delegate" ? delegateId || undefined : undefined,
      title: title || undefined,
      metadata: metadata || undefined,
      metaprotocol: metaprotocol || undefined,
      compressBr: compressBr && brotliEligible ? true : undefined,
      parentId: parentId || undefined,
      parentOutpoint: parentOutpoint || undefined,
      parentValue: parentValue ?? undefined,
      parentAddress: parentAddress || undefined,
      sameSatParent: sameSat || undefined,
      vaultAddress: address || undefined,
      ordinalsPublicKey: ordinalsPublicKey || undefined,
      satTarget: satForEngine,
      opReturn: opReturn || undefined,
      vanityPrefix: vanityPrefix || undefined,
      vanitySuffix: vanitySuffix || undefined,
      commitVanityPrefix: commitVanityPrefix || undefined,
      commitVanitySuffix: commitVanitySuffix || undefined,
      commitFeeRate:
        inscribePace === "fast"
          ? parseFeeRateInput(revealFeeRate)
          : parseFeeRateInput(commitFeeRate),
      revealFeeRate: parseFeeRateInput(revealFeeRate),
      /** Keep legacy field = reveal rate for older API paths / bundle readers */
      feeRate: parseFeeRateInput(revealFeeRate),
      postage: postageLocked
        ? Number(satResolvedValue)
        : Number(postage) || 546,
      destination: address || undefined,
      // Prefer wallet payment address (native or nested) — not ordinals taproot.
      paymentAddress: paymentAddress || undefined,
    };
  }, [
    mode,
    network,
    body,
    uploadB64,
    uploadType,
    uploadName,
    delegateId,
    title,
    metadata,
    metaprotocol,
    compressBr,
    brotliEligible,
    parentId,
    parentOutpoint,
    parentValue,
    parentAddress,
    ordinalsPublicKey,
    satTarget,
    satResolvedOutpoint,
    opReturn,
    vanityPrefix,
    vanitySuffix,
    commitVanityPrefix,
    commitVanitySuffix,
    commitFeeRate,
    revealFeeRate,
    inscribePace,
    postage,
    postageLocked,
    satResolvedValue,
    address,
    paymentAddress,
  ]);

  // Live network cost: debounce prepare (dry-run) as the plan changes.
  useEffect(() => {
    if (!address) {
      setNetworkCost({ status: "idle", detail: "Connect wallet to estimate network cost" });
      return;
    }
    if (isMainnetNet && !ordinalsPublicKey) {
      setNetworkCost({
        status: "error",
        detail: "Reconnect wallet (need Ordinals public key for self-custody estimate)",
      });
      return;
    }
    const hasContent =
      (mode === "text" && body.trim().length > 0) ||
      (mode === "upload" && Boolean(uploadB64)) ||
      (mode === "delegate" && delegateId.trim().length > 0);
    if (!hasContent) {
      setNetworkCost({ status: "idle", detail: "Add inscription content to estimate cost" });
      return;
    }

    let cancelled = false;
    setNetworkCost((prev) => ({ ...prev, status: "loading" }));
    const t = window.setTimeout(() => {
      void (async () => {
        try {
          const prep = await api.prepareInscription(plan);
          if (cancelled) return;
          if (prep.error && !(cliScalar(prep.commit_sats) || cliScalar(prep.commit_address))) {
            setNetworkCost({
              status: "error",
              detail: String(prep.error).slice(0, 180),
            });
            return;
          }
          const commitSats = cliNumber(prep.commit_sats);
          const postageSats = cliNumber(prep.postage_sats) || cliNumber(postage);
          const revealFeeSats = cliNumber(prep.reveal_fee_sats);
          const commitFeeSats =
            cliNumber(prep.commit_fee_estimate_sats) ||
            Math.ceil(
              estimateCommitVbytesInscribe({
                carrier: Boolean(
                  satTarget.trim() || satResolvedOutpoint || plan.sameSatParent
                ),
                paymentType: payType,
              }) * parseFeeRateInput(commitFeeRate)
            );
          const networkFeeSats =
            cliNumber(prep.network_fee_sats) ||
            (revealFeeSats + commitFeeSats);
          setNetworkCost({
            status: "ok",
            networkFeeSats: networkFeeSats || undefined,
            revealFeeSats: revealFeeSats || undefined,
            commitFeeSats: commitFeeSats || undefined,
            postageSats: postageSats || undefined,
            fundCommitSats:
              commitSats > 0
                ? commitSats
                : postageSats + revealFeeSats > 0
                  ? postageSats + revealFeeSats
                  : undefined,
            commitAddress: cliScalar(prep.commit_address) || undefined,
            detail: undefined,
          });
        } catch (e) {
          if (cancelled) return;
          setNetworkCost({ status: "error", detail: String(e).slice(0, 180) });
        }
      })();
    }, 450);
    return () => {
      cancelled = true;
      window.clearTimeout(t);
    };
  }, [
    address,
    isMainnetNet,
    ordinalsPublicKey,
    mode,
    body,
    uploadB64,
    delegateId,
    plan,
    postage,
    commitFeeRate,
    satTarget,
    satResolvedOutpoint,
    plan.sameSatParent,
    payType,
  ]);

  const sameSatParent = Boolean(plan.sameSatParent);
  const parentChildDifferentSat = Boolean(
    parentId.trim() && parentOutpoint && !sameSatParent
  );

  const contentPreview = useMemo(() => {
    if (mode === "text") {
      const looksHtml = /^\s*</.test(body) || /<\/?[a-z][\s\S]*>/i.test(body);
      return { kind: looksHtml ? ("html" as const) : ("text" as const), text: body };
    }
    if (mode === "upload" && uploadB64) {
      const ct = (uploadType || "").toLowerCase();
      if (ct.startsWith("text/") || ct.includes("svg") || ct.includes("html")) {
        try {
          const text = atob(uploadB64);
          const looksHtml = ct.includes("html") || ct.includes("svg") || /^\s*</.test(text);
          return { kind: looksHtml ? ("html" as const) : ("text" as const), text };
        } catch {
          return { kind: "binary" as const, text: uploadName || "file" };
        }
      }
      return { kind: "binary" as const, text: `${uploadName || "file"} (${uploadType || "binary"})` };
    }
    if (mode === "delegate") {
      return { kind: "text" as const, text: `Delegate → ${delegateId || "(set ID)"}` };
    }
    return null;
  }, [mode, body, uploadB64, uploadType, uploadName, delegateId]);

  const runRevealFromCommit = useCallback(
    async (
      commitTxid: string,
      expectedCommitAddress: string | undefined,
      planOverride?: InscribePlan,
      opts?: { signal?: AbortSignal }
    ) => {
      const activePlan = planOverride || plan;
      const sameSat = Boolean(activePlan.sameSatParent);
      const differentSat = Boolean(
        activePlan.parentId && activePlan.parentOutpoint && !sameSat
      );

      if (!address) {
        throw new Error("Connect wallet first");
      }
      if (isMainnetNet && !(activePlan.ordinalsPublicKey || ordinalsPublicKey)) {
        throw new Error(
          "Mainnet self-custody requires your Ordinals public key. Reconnect Xverse (Ordinals + Payment)."
        );
      }
      if (activePlan.parentId) {
        if (!sameSat && (!activePlan.parentOutpoint || activePlan.parentValue == null)) {
          throw new Error("Parent missing verified outpoint — cannot finish reveal.");
        }
        if (!activePlan.ordinalsPublicKey && !ordinalsPublicKey) {
          throw new Error(
            "Missing ordinals public key — reconnect Xverse (Ordinals + Payment)."
          );
        }
      }

      // Guard: form must regenerate the same commit address that was funded.
      if (!planOverride) {
        const prep = await api.prepareInscription(activePlan);
        if (prep.error) {
          throw new Error(String(prep.error));
        }
        const nowAddr = cliScalar(prep.commit_address);
        const expect = expectedCommitAddress || "";
        if (expect && nowAddr && expect !== nowAddr) {
          throw new Error(
            [
              "Inscription fields no longer match the funded commit.",
              `Funded commit address: ${expect}`,
              `Current form produces: ${nowAddr}`,
              `Commit tx: ${commitTxid}`,
              "",
              "Restore the exact body / parent / title / metaprotocol / compression used when you funded.",
            ].join("\n")
          );
        }
      }

      const hasRevealVanity = Boolean(
        activePlan.vanityPrefix || activePlan.vanitySuffix
      );
      setFlowPhase("grinding");
      const grindMsg = hasRevealVanity
        ? `Grinding reveal TXID ${activePlan.vanityPrefix || ""}…${activePlan.vanitySuffix || ""} — wait (can take ~1 min). Do not click again.`
        : "Building reveal PSBT…";
      setGrindNote(grindMsg);
      setOut(`Commit broadcast: ${commitTxid}\n${grindMsg}`);

      let rev: Awaited<ReturnType<typeof api.revealInscription>> | null = null;
      for (let attempt = 0; attempt < 5; attempt++) {
        if (opts?.signal?.aborted) {
          throw new Error("Reveal cancelled.");
        }
        rev = await api.revealInscription(
          {
            ...activePlan,
            commitTxid,
            destination: activePlan.destination || address,
            parentOutpoint: sameSat ? undefined : activePlan.parentOutpoint,
            parentValue: sameSat ? undefined : activePlan.parentValue,
            parentAddress: activePlan.parentAddress || address,
            vaultAddress: activePlan.vaultAddress || address,
            ordinalsPublicKey:
              activePlan.ordinalsPublicKey || ordinalsPublicKey || undefined,
            sameSatParent: sameSat || undefined,
          },
          { signal: opts?.signal }
        );
        if (!rev.error) break;
        const err = String(rev.error);
        if (attempt < 4 && isTransientError(err)) {
          setOut(
            `Commit funded: ${commitTxid}\nReveal attempt ${attempt + 1} failed (${err}) — retrying…`
          );
          await sleep(1200 * (attempt + 1));
          continue;
        }
        throw new Error(
          `${err}\n\nCommit unspent: ${commitTxid}\nOpen Profile if this session cannot finish.`
        );
      }
      if (!rev || rev.error) {
        throw new Error(`Reveal failed.\nCommit unspent: ${commitTxid}`);
      }

      const walletRevealPsbt = Boolean(rev.psbt_base64) && !rev.reveal_txid && !rev.broadcast_txid;
      let revealTxid = "";

      if (differentSat && !rev.psbt_base64 && !rev.reveal_txid && !rev.broadcast_txid) {
        throw new Error(
          [
            "Parent-child reveal did not return a wallet PSBT — cannot spend parent.",
            `Commit unspent: ${commitTxid}`,
            formatResult(rev),
          ].join("\n")
        );
      }

      if (walletRevealPsbt) {
        const inspected = await api.inspectPsbt({
          base64: String(rev.psbt_base64),
          network: activePlan.network || network,
        });
        const custodyNote = Boolean(activePlan.ordinalsPublicKey || ordinalsPublicKey);
        const go = await requestSignReview({
          kind: "reveal",
          headline: differentSat
            ? "Review reveal (parent spend) before wallet sign"
            : "Review reveal before wallet sign",
          steps: [
            `Commit already broadcast: ${commitTxid}`,
            differentSat
              ? "This PSBT spends the parent UTXO (vin0) + commit (vin1)."
              : "This PSBT spends the commit via inscription tapscript.",
            String(rev.parent_lands || ""),
            String(rev.child_lands || "Inscription lands on postage output."),
            custodyNote
              ? "Self-custody: tapscript uses your wallet pubkey — only you can sign/reveal/recover."
              : "Sign the required inputs in your wallet.",
            "Sign = authorize this reveal. We broadcast it immediately after you approve in Xverse.",
          ].filter(Boolean),
          warnings: isMainnetNet
            ? ["Mainnet — real BTC. Wallet will show the same inputs/outputs."]
            : [],
          inputs: Array.isArray(inspected.vin) ? inspected.vin : [],
          outputs: Array.isArray(inspected.vout) ? inspected.vout : [],
          meta: [
            inspected.fee_sats ? `fee ≈ ${inspected.fee_sats} sats` : "",
            inspected.unsigned_txid ? `preview txid ${inspected.unsigned_txid}` : "",
            rev.vanity_reveal_txid
              ? `predicted reveal TXID ${rev.vanity_reveal_txid} (not broadcast yet)`
              : "",
          ].filter(Boolean),
          buttonLabel: "Sign & broadcast reveal",
        });
        if (!go) {
          throw new Error(
            `Cancelled before reveal sign.\nCommit unspent: ${commitTxid}\nOpen Profile → Send reveal to finish later.`
          );
        }
        setSignReview(null);

        // Parent FI/FO: vin0 = parent. Wallet custody single-input: vin0 = commit.
        // Sign all inputs owned by the ordinals address.
        const signIdx: number[] = differentSat ? [0, 1] : [0];
        // If keystore pre-signed commit (vin1), wallet only needs parent (vin0).
        const parentOnly =
          differentSat &&
          !custodyNote &&
          String(rev.note || rev.fields?.note || "").toLowerCase().includes("keystore");
        const indices = parentOnly ? [0] : differentSat && custodyNote ? [0, 1] : signIdx;

        let signedPsbt = "";
        for (let round = 1; round <= 2; round++) {
          setOut(
            [
              `Commit broadcast: ${commitTxid}`,
              String(rev.parent_lands || ""),
              String(rev.child_lands || ""),
              round === 1
                ? differentSat
                  ? "Sign reveal in wallet (parent vin0 + commit vin1) — then we broadcast…"
                  : "Sign reveal in wallet — then we broadcast…"
                : `Wallet cancelled — prompt ${round}: approve sign to broadcast reveal (commit already on network).`,
            ]
              .filter(Boolean)
              .join("\n")
          );
          // Wallet signs only; Phechan broadcasts immediately after (Xverse broadcast is unreliable).
          const signedReveal = await signPsbtWithWallet(String(rev.psbt_base64), {
            broadcast: false,
            signInputs: { [address]: indices },
          });
          if (signedReveal.ok && signedReveal.psbt) {
            signedPsbt = signedReveal.psbt;
            break;
          }
          const msg = signedReveal.message || "Wallet did not sign reveal";
          if (isWalletCancel(msg) && round < 2) {
            continue;
          }
          throw new Error(`${msg}\n\nCommit unspent: ${commitTxid}`);
        }

        setOut("Wallet signed — broadcasting reveal now…");
        let fin: Awaited<ReturnType<typeof api.finalizeFundingPsbt>> | null = null;
        for (let attempt = 0; attempt < 5; attempt++) {
          fin = await api.finalizeFundingPsbt({
            base64: signedPsbt,
            network: activePlan.network || network,
            broadcast: true,
          });
          revealTxid = String(fin.broadcast_txid || "");
          if (fin.ok && revealTxid) break;
          const err = String(fin.error || fin.stderr || "Reveal broadcast failed");
          if (attempt < 4 && isTransientError(err)) {
            setOut(`Broadcast retry ${attempt + 1}… (${err})`);
            await sleep(1200 * (attempt + 1));
            continue;
          }
          throw new Error(`${err}\n\nCommit unspent: ${commitTxid}\n${formatResult(fin)}`);
        }
      } else {
        revealTxid = String(rev.reveal_txid || rev.broadcast_txid || "");
        if (!revealTxid && rev.vanity_reveal_txid && !rev.psbt_base64) {
          throw new Error(
            `Reveal returned vanity preview ${rev.vanity_reveal_txid} but no broadcast_txid — not sent.\nCommit unspent: ${commitTxid}\n${formatResult(rev)}`
          );
        }
      }

      const inscriptionId = String(
        rev.inscription_id_guess ||
          rev.child_inscription_id_guess ||
          (revealTxid ? `${revealTxid}i0` : "")
      );
      if (!revealTxid) {
        throw new Error(
          `Reveal completed without txid.\nCommit unspent: ${commitTxid}\n${formatResult(rev)}`
        );
      }

      removeInterruptedCommit(commitTxid);
      refreshInterrupted();
      setPendingReveal(null);
      setSignReview(null);
      setFlowPhase("done");
      setResultTx({ commitTxid, revealTxid, inscriptionId });
      setOk(true);
      setOut(
        [
          activePlan.parentOutpoint || activePlan.parentId
            ? sameSat
              ? "Child inscribed on parent sat (reinscription + provenance tag 3)."
              : "Child inscribed (parent spent + returned)."
            : "Inscribed.",
          `commit: ${commitTxid}`,
          `reveal: ${revealTxid}`,
          `inscription: ${inscriptionId}`,
          sameSat
            ? `parent+child on same sat → ${activePlan.destination || address}`
            : activePlan.parentOutpoint
              ? `parent returned → vout${activePlan.opReturn ? 1 : 0} ${activePlan.destination || address} (${activePlan.parentValue} sats)`
              : "",
          explorerTxUrl(activePlan.network || network, revealTxid),
        ]
          .filter(Boolean)
          .join("\n")
      );
    },
    [
      address,
      plan,
      ordinalsPublicKey,
      network,
      isMainnetNet,
      requestSignReview,
      refreshInterrupted,
    ]
  );

  const runInscribe = useCallback(async () => {
    if (!address) {
      setOk(false);
      setOut("Connect wallet first — need your address for funding + reveal destination.");
      setFlowPhase("error");
      return;
    }
    if (mode === "delegate" && !delegateId.trim()) {
      setOk(false);
      setOut("Delegate ID required.");
      setFlowPhase("error");
      return;
    }
    if (isMainnetNet && !ordinalsPublicKey) {
      setOk(false);
      setOut(
        "Mainnet self-custody requires your Ordinals public key. Connect/reconnect Xverse (Ordinals + Payment)."
      );
      setFlowPhase("error");
      return;
    }

    setBusy(true);
    setOk(null);
    setResultTx(null);
    setSignReview(null);
    setFlowPhase("preparing");
    setOut("Preparing commit address + postage/fee…");
    setGrindNote("");

    try {
      const prep = await api.prepareInscription(plan);
      if (prep.error && !(cliScalar(prep.commit_address) && cliNumber(prep.commit_sats))) {
        throw new Error(String(prep.error));
      }
      const commitAddress = cliScalar(prep.commit_address);
      const commitSats = cliNumber(prep.commit_sats);
      if (!commitAddress || !commitSats) {
        throw new Error(
          `Prepare failed — missing commit_address/commit_sats.\n${formatResult(prep)}`
        );
      }

      setFlowPhase("funding");
      const payAddr = paymentAddress || address;
      if (parentId.trim()) {
        if (parentVerify !== "ok" || !parentOutpoint || parentValue == null) {
          throw new Error(
            "Parent set but not Verified. Click Verify — we must spend the parent UTXO for real provenance (tag 3 alone is not enough)."
          );
        }
        if (!ordinalsPublicKey) {
          throw new Error(
            "Missing ordinals public key. Disconnect/reconnect Xverse (Ordinals + Payment)."
          );
        }
        if (sameSatParent) {
          const satOk =
            (satResolvedOutpoint &&
              satResolvedOutpoint.toLowerCase() === parentOutpoint.toLowerCase()) ||
            satTarget.trim().toLowerCase() === parentId.trim().toLowerCase();
          if (!satOk) {
            throw new Error(
              "Same-sat child: put the parent id (or its sat) in Target sat / inscription and Verify so both resolve to the same UTXO."
            );
          }
        }
      }
      const fundingKind = paymentAddressType(payAddr);
      if (/^[mn1]/i.test(payAddr) && !/^(tb1|bc1|bcrt1|2|3)/i.test(payAddr)) {
        throw new Error(
          "Legacy P2PKH payment address not supported. Use nested (2…/3…) or native (tb1q/bc1q) segwit."
        );
      }
      if (fundingKind === "p2sh-p2wpkh" && !paymentPublicKey) {
        throw new Error(
          "Nested segwit payment needs paymentPublicKey from wallet. Disconnect/reconnect Xverse, then try again."
        );
      }

      const feeR =
        inscribePace === "fast"
          ? parseFeeRateInput(revealFeeRate)
          : parseFeeRateInput(commitFeeRate);
      // Reinscription / same-sat: inscription UTXO must be vin0 so the sat moves into commit.
      // Payment alone only creates a NEW sat inscription — sat target would be ignored.
      const useSatCarrier = Boolean(
        sameSatParent || (satVerify === "ok" && satResolvedOutpoint)
      );
      if (satTarget.trim() && !useSatCarrier) {
        throw new Error(
          "Target sat / inscription is set but not Verified. Click Verify so we can spend that UTXO as vin0 (otherwise Xverse only sees your payment UTXO)."
        );
      }
      if (useSatCarrier && !ordinalsPublicKey) {
        throw new Error(
          "Missing ordinals public key. Disconnect/reconnect Xverse (Ordinals + Payment) — needed to sign the inscription sat UTXO."
        );
      }

      const carrierOutpoint = sameSatParent ? parentOutpoint : satResolvedOutpoint;
      const [carrierTxid, carrierVoutStr] = carrierOutpoint
        ? carrierOutpoint.split(":")
        : ["", ""];
      const carrierVout = Number(carrierVoutStr);
      const carrierValueSats = sameSatParent
        ? parentValue ?? 0
        : satResolvedValue ?? 0;
      if (useSatCarrier && (!carrierTxid || !Number.isFinite(carrierVout))) {
        throw new Error("Sat/parent outpoint missing — Verify again.");
      }
      if (useSatCarrier && carrierValueSats <= 0) {
        throw new Error(
          "Carrier UTXO value unknown. Re-Verify the target sat (needs value from ord/esplora)."
        );
      }

      // Carrier vin0; payment top-up only if carrier cannot cover commit+fee.
      let needPaymentTopUp = true;
      let utxo:
        | { txid: string; vout: number; value: number; scriptPubKey?: string; address?: string }
        | undefined;

      if (useSatCarrier) {
        const roughFundVb = 120; // p2tr in + commit + change
        const roughFee = Math.max(1, Math.round(roughFundVb * feeR));
        needPaymentTopUp = carrierValueSats < commitSats + roughFee;
        setOut(
          needPaymentTopUp
            ? `Sat carrier ${carrierOutpoint} (${carrierValueSats} sats) + payment top-up → commit ${commitSats}`
            : `Funding commit from inscription sat UTXO ${carrierOutpoint} (${carrierValueSats} sats) alone`
        );
        if (needPaymentTopUp) {
          let utxos = fundingUtxos;
          if (!utxos.length) {
            const u = await getWalletUtxos(payAddr, network);
            if (!u.ok || !u.utxos.length) {
              throw new Error(
                u.message ||
                  `Carrier alone is too small (${carrierValueSats} < ${commitSats}+fee). Need a payment UTXO on ${payAddr}.`
              );
            }
            utxos = u.utxos;
            setFundingUtxos(u.utxos);
          }
          const needPay = commitSats + roughFee - carrierValueSats + 200;
          utxo = selectedUtxoKey
            ? utxos.find((x) => `${x.txid}:${x.vout}` === selectedUtxoKey)
            : undefined;
          if (!utxo) utxo = pickFundingUtxo(utxos, needPay, feeR, payAddr) || undefined;
          if (!utxo) {
            throw new Error(
              `Carrier ${carrierValueSats} sats + no payment UTXO large enough for top-up (~${needPay} sats)`
            );
          }
          setSelectedUtxoKey(`${utxo.txid}:${utxo.vout}`);
        }
      } else {
        setOut(`Fetching UTXOs for payment (${fundingKind}) on ${network}…`);
        let utxos = fundingUtxos;
        if (!utxos.length) {
          const u = await getWalletUtxos(payAddr, network);
          if (!u.ok || !u.utxos.length) {
            throw new Error(
              u.message ||
                `No UTXOs on ${payAddr}. Check wallet is on ${network} and payment address is funded.`
            );
          }
          utxos = u.utxos;
          setFundingUtxos(u.utxos);
        }
        utxo = selectedUtxoKey
          ? utxos.find((x) => `${x.txid}:${x.vout}` === selectedUtxoKey)
          : undefined;
        if (!utxo) utxo = pickFundingUtxo(utxos, commitSats, feeR, payAddr) || undefined;
        if (!utxo) {
          throw new Error("No funding UTXO large enough for commit + fee");
        }
        setSelectedUtxoKey(`${utxo.txid}:${utxo.vout}`);
      }

      if (commitVanityPrefix || commitVanitySuffix) {
        setGrindNote(
          `Grinding commit TXID ${commitVanityPrefix || ""}…${commitVanitySuffix || ""} (${commitVanityDiff.description})`
        );
      }

      const fundPsbt = await api.fundCommitPsbt({
        ...plan,
        commitAddress,
        commitSats,
        paymentAddress: payAddr,
        paymentPublicKey: paymentPublicKey || undefined,
        changeAddress: payAddr,
        ...(useSatCarrier
          ? {
              carrierTxid,
              carrierVout,
              carrierValue: carrierValueSats,
              carrierAddress: (sameSatParent ? parentAddress : undefined) || address,
              ordinalsPublicKey: ordinalsPublicKey || undefined,
              ...(needPaymentTopUp && utxo
                ? {
                    fundingTxid: utxo.txid,
                    fundingVout: utxo.vout,
                    fundingValue: utxo.value,
                    fundingScriptHex: utxo.scriptPubKey,
                    fundingAddress: utxo.address || payAddr,
                  }
                : {}),
            }
          : {
              fundingTxid: utxo!.txid,
              fundingVout: utxo!.vout,
              fundingValue: utxo!.value,
              fundingScriptHex: utxo!.scriptPubKey,
              fundingAddress: utxo!.address || payAddr,
            }),
        commitVanityPrefix: commitVanityPrefix || undefined,
        commitVanitySuffix: commitVanitySuffix || undefined,
      });
      if (fundPsbt.error || !fundPsbt.psbt_base64) {
        const detail =
          fundPsbt.error ||
          fundPsbt.stderr ||
          (Array.isArray(fundPsbt.raw) ? fundPsbt.raw.join("\n") : "") ||
          "fund-psbt failed — no psbt_base64";
        throw new Error(String(detail));
      }

      const signInputs: Record<string, number[]> = useSatCarrier
        ? needPaymentTopUp
          ? { [address]: [0], [payAddr]: [1] }
          : { [address]: [0] }
        : { [payAddr]: [0] };

      const inspected = await api.inspectPsbt({
        base64: fundPsbt.psbt_base64,
        network,
      });

      const modeLabel =
        mode === "delegate"
          ? `delegate ${delegateId.trim()}`
          : mode === "upload"
            ? `file ${uploadName || "upload"}`
            : "text body";
      const steps = [
        `Network: ${network}${isMainnetNet ? " (real BTC)" : ""}`,
        `Content: ${modeLabel}`,
        parentId.trim()
          ? sameSatParent
            ? `Same-sat child on parent ${parentId.trim()} — parent UTXO funds commit${
                isControlPace
                  ? "; reveal later when you choose."
                  : "; sign #1 broadcasts commit, sign #2 reveals."
              }`
            : `Different-sat child of ${parentId.trim()} — sign #1 funds+broadcasts commit (Payment)${
                isControlPace
                  ? "; reveal later signs parent (Ordinals)."
                  : "; sign #2 spends parent+commit (Ordinals)."
              }`
          : useSatCarrier
            ? `Reinscription on sat UTXO ${carrierOutpoint} — ordinals vin0 moves the sat into commit${needPaymentTopUp ? "; payment top-up vin1" : ""}${
                isControlPace ? "." : "; then sign #2 reveals."
              }`
            : isControlPace
              ? "Standalone inscription — fund commit now; reveal when you choose (bundle auto-downloads)."
              : ordinalsPublicKey
                ? "Standalone — sign #1 funds+broadcasts commit (Payment); sign #2 reveals (Ordinals). Not parent-child."
                : "Standalone inscription — one funding sign, then automatic reveal.",
        `Commit output: ${commitSats} sats → ${commitAddress}`,
        isControlPace
          ? `Commit fee ${feeR} sat/vB · reveal budget ${parseFeeRateInput(revealFeeRate)} sat/vB · postage ${postage} sats`
          : `Fee rate ${feeR} sat/vB · postage ${postage} sats`,
        useSatCarrier
          ? needPaymentTopUp
            ? "Wallet will sign Ordinals (vin0 / inscription sat) + Payment (vin1) in one prompt."
            : "Wallet will sign Ordinals (inscription sat as vin0) only."
          : "Wallet will sign Payment funding input.",
        "Each wallet sign is broadcast immediately after you approve in Xverse (via your local node).",
        ...(isControlPace
          ? [
              "After commit confirms in mempool/chain, a .phechan.json bundle downloads — reveal is not automatic.",
            ]
          : []),
      ];
      const go = await requestSignReview({
        kind: "funding",
        headline: isMainnetNet
          ? "Review funding transaction (mainnet)"
          : "Review funding transaction",
        steps,
        warnings: isMainnetNet
          ? [
              "Mainnet broadcast unlocked on API. Approving signs real BTC — Xverse shows the same I/O.",
            ]
          : [],
        inputs: Array.isArray(inspected.vin) ? inspected.vin : [],
        outputs: Array.isArray(inspected.vout) ? inspected.vout : [],
        meta: [
          `fee ${fundPsbt.commit_funding_fee_sats || inspected.fee_sats || "?"} sats`,
          fundPsbt.commit_vanity_txid
            ? `commit vanity TXID ${fundPsbt.commit_vanity_txid}`
            : inspected.unsigned_txid
              ? `preview txid ${inspected.unsigned_txid}`
              : "",
        ].filter(Boolean),
        buttonLabel: isControlPace
          ? "Sign & broadcast commit"
          : parentChildDifferentSat
            ? "Sign funding & continue"
            : "Sign & Inscribe",
      });
      if (!go) {
        setOk(null);
        return;
      }
      setSignReview(null);

      setOut(
        (useSatCarrier
          ? needPaymentTopUp
            ? "Sign once: Ordinals (inscription sat vin0) + Payment (fee vin1)…\n"
            : "Sign once: Ordinals — inscription sat moves into commit…\n"
          : parentChildDifferentSat
            ? "Sign #1 of 2: Payment funds commit (Ordinals signs parent at reveal)…\n"
            : ordinalsPublicKey
              ? "Sign #1 of 2: Payment funds commit (Ordinals signs reveal next)…\n"
              : "Sign funding in wallet…\n") +
          `(${fundPsbt.commit_funding_fee_sats || "?"} sat fee @ ${feeR} sat/vB)` +
          (fundPsbt.commit_vanity_txid ? `\nCommit vanity TXID ${fundPsbt.commit_vanity_txid}` : "")
      );

      let signedPsbt = "";
      let signedTxid: string | undefined;
      for (let round = 1; ; round++) {
        if (round > 1) {
          setOut(
            `Wallet cancelled funding sign — prompt ${round}: approve to continue Inscribe (nothing broadcast yet)…`
          );
        }
        const signed = await signPsbtWithWallet(fundPsbt.psbt_base64, {
          broadcast: false,
          signInputs,
        });
        if (signed.ok && (signed.psbt || signed.txid)) {
          signedPsbt = signed.psbt || "";
          signedTxid = signed.txid;
          break;
        }
        const msg = signed.message || "Wallet signing cancelled";
        if (isWalletCancel(msg)) continue;
        throw new Error(msg);
      }

      let commitTxid = "";
      // Rule: every wallet sign is broadcast immediately (via local node / Esplora).
      // We never hold a signed commit or reveal for later packaging.
      if (signedPsbt) {
        setOut("Wallet signed — broadcasting commit now…");
        const fin = await api.finalizeFundingPsbt({
          base64: signedPsbt,
          network,
          broadcast: true,
          paymentPublicKey: paymentPublicKey || undefined,
        });
        const broadcasted = String(fin.broadcast_txid || signedTxid || "");
        if (!fin.ok || !broadcasted) {
          throw new Error(
            String(
              fin.error ||
                fin.stderr ||
                `Funding broadcast failed.\n${formatResult(fin)}`
            )
          );
        }
        commitTxid = broadcasted;
      } else if (signedTxid) {
        commitTxid = signedTxid;
      }
      if (!commitTxid) {
        throw new Error(
          "Wallet signed but returned no PSBT/txid — cannot continue funding"
        );
      }

      // Snapshot so Profile can finish if reveal fails / is deferred.
      // Always pin the payment address that funded the commit — reveal surplus
      // (lower fee later) returns change there, not to ordinals.
      const savedPlan = snapshotPlan({
        ...plan,
        network,
        destination: address,
        vaultAddress: address,
        parentAddress: parentAddress || address,
        ordinalsPublicKey: ordinalsPublicKey || undefined,
        sameSatParent: sameSatParent || undefined,
        paymentAddress: payAddr,
        commitTxid,
        commitVout: 0,
        commitValue: commitSats,
      });
      const interruptedItem: InterruptedCommit = {
        commitTxid,
        commitAddress,
        network,
        savedAt: Date.now(),
        plan: savedPlan,
      };
      upsertInterruptedCommit(interruptedItem);
      refreshInterrupted();

      if (isControlPace) {
        setPendingReveal(interruptedItem);
        const bundle = createCommitBundle({
          commitTxid,
          commitAddress,
          network,
          plan: savedPlan,
          commitSats,
        });
        try {
          downloadCommitBundle(bundle);
        } catch (dlErr) {
          console.warn("commit bundle auto-download failed", dlErr);
        }

        setResultTx({ commitTxid });
        setFlowPhase("committed");
        setOk(true);
        setOut(
          [
            `Commit broadcast: ${commitTxid}`,
            `Bundle downloaded — keep the .phechan.json file safe.`,
            `Reveal fee was pre-funded at ~${parseFeeRateInput(revealFeeRate)} sat/vB (commit funded at ~${feeR} sat/vB).`,
            `Reveal when ready: Reveal now below, Profile → Send reveal, or Upload bundle on Profile.`,
          ].join("\n")
        );
        return;
      }

      // Fast: commit is already broadcast; chain reveal sign → broadcast in this session.
      setPendingReveal(null);
      setOut(
        useSatCarrier
          ? `Commit broadcast: ${commitTxid}\nReveal — target sat already in commit…`
          : `Commit broadcast: ${commitTxid}\nFinishing reveal (fast mode)…`
      );
      try {
        await runRevealFromCommit(commitTxid, commitAddress, {
          ...plan,
          paymentAddress: payAddr,
          commitTxid,
          commitVout: 0,
          commitValue: commitSats,
          destination: address,
          vaultAddress: address,
          ordinalsPublicKey: ordinalsPublicKey || plan.ordinalsPublicKey,
        });
      } catch (revealErr) {
        refreshInterrupted();
        throw new Error(
          `${String(revealErr)}\n\nCommit is broadcast. Open Profile → Send reveal to finish without funding again.`
        );
      }
    } catch (e) {
      setFlowPhase("error");
      setOk(false);
      setOut(String(e));
    } finally {
      setBusy(false);
      setGrindNote("");
    }
  }, [
    address,
    paymentAddress,
    paymentPublicKey,
    ordinalsPublicKey,
    network,
    mode,
    delegateId,
    uploadName,
    plan,
    commitFeeRate,
    revealFeeRate,
    inscribePace,
    isControlPace,
    postage,
    fundingUtxos,
    selectedUtxoKey,
    commitVanityPrefix,
    commitVanitySuffix,
    commitVanityDiff,
    parentId,
    parentVerify,
    parentOutpoint,
    parentValue,
    parentAddress,
    sameSatParent,
    parentChildDifferentSat,
    satTarget,
    satResolvedOutpoint,
    satResolvedValue,
    satVerify,
    runRevealFromCommit,
    refreshInterrupted,
    isMainnetNet,
    requestSignReview,
  ]);

  /** Reveal-only overrides on a deferred commit (commit already funded — vanity/fee safe to change). */
  function patchInterruptedReveal(
    commitTxid: string,
    patch: Partial<Pick<InscribePlan, "vanityPrefix" | "vanitySuffix" | "revealFeeRate" | "feeRate">>
  ) {
    const cur = listInterruptedCommits().find((x) => x.commitTxid === commitTxid);
    if (!cur) return;
    const nextPlan: InscribePlan = { ...cur.plan, ...patch };
    if ("vanityPrefix" in patch && !patch.vanityPrefix) delete nextPlan.vanityPrefix;
    if ("vanitySuffix" in patch && !patch.vanitySuffix) delete nextPlan.vanitySuffix;
    upsertInterruptedCommit({
      ...cur,
      plan: snapshotPlan(nextPlan),
    });
    refreshInterrupted();
    if (pendingReveal?.commitTxid === commitTxid) {
      setPendingReveal({
        ...cur,
        plan: snapshotPlan(nextPlan),
        savedAt: Date.now(),
      });
    }
  }

  function cancelReveal() {
    revealAbortRef.current?.abort();
    revealAbortRef.current = null;
    if (signReviewResolver.current) {
      const resolve = signReviewResolver.current;
      signReviewResolver.current = null;
      resolve(false);
    }
    setSignReview(null);
    setBusy(false);
    setRevealingCommitTxid(null);
    setGrindNote("");
    setFlowPhase("error");
    setOut("Reveal cancelled. Commit is still funded — edit vanity if needed, then Send reveal again.");
  }

  async function sendRevealFromProfile(item: InterruptedCommit) {
    if (!address) {
      setOk(false);
      setOut("Connect wallet first");
      return;
    }
    if (busy) {
      setOut(
        grindNote ||
          "Reveal already in progress — wait for grind / wallet sign, or Cancel."
      );
      return;
    }
    // Clear any stuck prior sign-review waiter (e.g. Profile hang before modal was global).
    if (signReviewResolver.current) {
      const resolve = signReviewResolver.current;
      signReviewResolver.current = null;
      resolve(false);
      setSignReview(null);
    }
    // Re-read so Profile fee/vanity edits apply even if list row is stale.
    const latest =
      listInterruptedCommits().find((x) => x.commitTxid === item.commitTxid) || item;
    const commitNet = String(latest.network || latest.plan.network || "").toLowerCase();
    const walletNet = String(network || "").toLowerCase();
    if (commitNet && walletNet && commitNet !== walletNet) {
      setOk(false);
      setOut(
        `Network mismatch: commit is on ${commitNet}, wallet is on ${walletNet}. Switch wallet network (or reconnect) before Send reveal.`
      );
      return;
    }
    revealAbortRef.current?.abort();
    const ac = new AbortController();
    revealAbortRef.current = ac;
    setBusy(true);
    setOk(null);
    setRevealingCommitTxid(latest.commitTxid);
    // Stay on Profile — sign-review panel is now global (was Inscribe-only; that blocked popup).
    setPage("profile");
    setPendingReveal(latest);
    try {
      // Change from a lower reveal fee must go to the payment address that funded
      // the commit (saved in the bundle) — not the ordinals / vault address.
      const revealPayAddr =
        latest.plan.paymentAddress || paymentAddress || undefined;
      if (!revealPayAddr) {
        setOk(false);
        setBusy(false);
        setRevealingCommitTxid(null);
        setOut(
          "Missing payment address on this commit bundle. Re-download won’t help if it was never saved — reconnect the wallet that funded the commit (payment address), then Send reveal so surplus can return as change."
        );
        return;
      }
      const p: InscribePlan = {
        ...latest.plan,
        // Prefer interrupted/bundle network so reveal fee + vanity tip hit the right chain.
        network: latest.network || latest.plan.network || network,
        destination: address,
        vaultAddress: address,
        ordinalsPublicKey: ordinalsPublicKey || latest.plan.ordinalsPublicKey,
        paymentAddress: revealPayAddr,
        // Avoid slow commit tx lookup when we already know the funded outpoint.
        commitVout: latest.plan.commitVout ?? 0,
        commitValue: latest.plan.commitValue,
      };
      // Persist cleared vanity / fee edits + commitVout for next attempt
      upsertInterruptedCommit({
        ...latest,
        network: p.network || latest.network,
        plan: snapshotPlan(p),
      });
      await runRevealFromCommit(latest.commitTxid, latest.commitAddress, p, {
        signal: ac.signal,
      });
      setPendingReveal(null);
    } catch (e) {
      const msg = String(e);
      if (ac.signal.aborted || /cancelled|abort/i.test(msg)) {
        setFlowPhase("error");
        setOk(false);
        setOut(
          "Reveal cancelled. Commit is still funded — Clear vanity for a fast finish, then Send reveal."
        );
      } else {
        setFlowPhase("error");
        setOk(false);
        setOut(msg);
      }
      setPage("profile");
      refreshInterrupted();
    } finally {
      if (revealAbortRef.current === ac) revealAbortRef.current = null;
      setBusy(false);
      setRevealingCommitTxid(null);
      setGrindNote("");
    }
  }

  function redownloadBundle(item: InterruptedCommit) {
    const bundle = createCommitBundle({
      commitTxid: item.commitTxid,
      commitAddress: item.commitAddress,
      network: item.network,
      plan: item.plan,
      commitSats: item.plan.commitValue,
    });
    downloadCommitBundle(bundle);
    setOut(`Re-downloaded bundle for ${item.commitTxid.slice(0, 12)}…`);
  }

  async function onUploadCommitBundle(file: File | null) {
    if (!file) return;
    if (file.size > 2 * 1024 * 1024) {
      setOk(false);
      setOut("Bundle file too large (max 2 MB).");
      return;
    }
    try {
      const text = await file.text();
      const item = importCommitBundle(text);
      refreshInterrupted();
      setPendingReveal(item);
      setOk(true);
      setOut(
        [
          `Imported commit bundle: ${item.commitTxid}`,
          `Network: ${item.network}`,
          `Use Send reveal on Profile (or Reveal now) when ready.`,
        ].join("\n")
      );
      setPage("profile");
    } catch (e) {
      setOk(false);
      setOut(String(e));
    } finally {
      if (bundleFileRef.current) bundleFileRef.current.value = "";
    }
  }

  async function onConnect() {
    setBusy(true);
    const session = await connectWallet();
    setBusy(false);
    setOk(session.ok);
    setWalletNote(session.message);
    if (session.address) setAddress(session.address);
    if (session.paymentAddress) setPaymentAddress(session.paymentAddress);
    else if (session.address) setPaymentAddress(session.address);
    setPaymentPublicKey(session.paymentPublicKey || "");
    setOrdinalsPublicKey(session.ordinalsPublicKey || "");
    if (session.network) {
      setNetwork(session.network);
    }
    setFundingUtxos([]);
    setSelectedUtxoKey("");
    const pay = session.paymentAddress || session.address;
    const kind = paymentAddressType(pay);
    setOut(
      [
        session.message,
        session.address && `ordinals: ${session.address}`,
        pay && `payment: ${pay} (${kind})`,
        session.paymentPublicKey
          ? `paymentPublicKey: ${session.paymentPublicKey.slice(0, 16)}…`
          : kind === "p2sh-p2wpkh"
            ? "warning: no paymentPublicKey — reconnect required for nested segwit"
            : undefined,
        session.ordinalsPublicKey
          ? `ordinalsPublicKey: ${session.ordinalsPublicKey.slice(0, 16)}…`
          : "note: no ordinalsPublicKey — parent-child signing may fail; reconnect",
        session.network && `network: ${session.network}`,
      ]
        .filter(Boolean)
        .join("\n")
    );
  }

  async function onUpload(file: File | null) {
    if (!file) return;
    setUploadName(file.name);
    setUploadType(file.type || "application/octet-stream");
    setUploadB64(await fileToBase64(file));
  }

  async function verifyDelegate() {
    setDelegateVerify("loading");
    setDelegateMsg("");
    const r = await api.verifyDelegate(delegateId.trim(), network);
    if (r.ok) {
      setDelegateVerify("ok");
      setDelegateMsg(r.detail || "Delegate target found");
    } else {
      setDelegateVerify("error");
      setDelegateMsg(r.error || "Verify failed");
    }
  }

  async function verifyParent() {
    if (!address) {
      setParentVerify("error");
      setParentMsg("Connect wallet first");
      return;
    }
    setParentVerify("loading");
    setParentMsg("");
    setParentOutpoint("");
    setParentValue(null);
    setParentAddress("");
    const r = await api.verifyParent(parentId.trim(), address, network);
    if (r.ok && r.owned && r.txid != null && r.vout != null) {
      setParentVerify("ok");
      setParentOutpoint(`${r.txid}:${r.vout}`);
      setParentValue(typeof r.value === "number" ? r.value : Number(r.value) || null);
      setParentAddress(String(r.address || address));
      const val = typeof r.value === "number" ? r.value : Number(r.value) || "?";
      setParentMsg(
        [
          r.detail || "Verified — you hold this parent",
          `Outpoint: ${r.txid}:${r.vout} (${val} sats)`,
          "",
          "Placement is inferred from Target sat / inscription below:",
          "• Same id or same UTXO → child on parent sat (fund moves parent into commit; usually one wallet sign)",
          "• Parent only / different UTXO → child on new postage sat (FI/FO; sign payment then ordinals)",
        ].join("\n")
      );
    } else {
      setParentVerify("error");
      setParentOutpoint("");
      setParentValue(null);
      setParentMsg(r.error || r.detail || "Not verified");
    }
  }

  async function verifySat() {
    if (!address) {
      setSatVerify("error");
      setSatMsg("Connect wallet first");
      return;
    }
    setSatVerify("loading");
    setSatMsg("");
    setSatReinscribe(false);
    setSatResolvedOutpoint("");
    setSatResolvedValue(null);
    const r = await api.verifySatTarget(satTarget.trim(), address, network);
    if (r.ok && r.owned) {
      setSatVerify("ok");
      setSatMsg(r.detail || "Verified");
      setSatReinscribe(Boolean(r.reinscribe));
      if (r.txid != null && r.vout != null) setSatResolvedOutpoint(`${r.txid}:${r.vout}`);
      const val = typeof r.value === "number" ? r.value : Number(r.value);
      if (Number.isFinite(val)) {
        setSatResolvedValue(val);
        if (r.reinscribe) setPostage(String(val));
      } else {
        setSatResolvedValue(null);
      }
    } else {
      setSatVerify("error");
      setSatMsg(r.error || r.detail || "Not verified");
    }
  }

  return (
    <div className={page === "studio" ? "app app-studio" : "app"}>
      <header className="top">
        <div>
          <h1 className="brand">Phechan</h1>
          <p className="tagline">Your Bitcoin. Your Sats. Your Protocol.</p>
        </div>
        <div className="wallet-card">
          <div className="wallet-row">
            <button type="button" className="primary" disabled={busy || !walletReady} onClick={onConnect}>
              {address ? "Refresh wallet" : "Connect wallet"}
            </button>
            {address ? (
              <span
                className={`network-pill live net-${network.toLowerCase()}`}
                title="Network from connected wallet"
              >
                {network}
              </span>
            ) : null}
          </div>
          <p className="wallet-meta">{walletNote}</p>
          {address && (
            <p className="wallet-addr" title={address}>
              ordinals {address}
            </p>
          )}
          {paymentAddress && (
            <p className="wallet-meta" title={paymentAddress}>
              payment {paymentAddress}{" "}
              <span className="muted">({payType})</span>
            </p>
          )}
        </div>
      </header>

      <div className="banner">Local only · keys stay in wallet · verify holdings before build</div>

      <nav className="mode-row" style={{ marginBottom: "1rem" }}>
        <button
          type="button"
          className={page === "inscribe" ? "mode active" : "mode"}
          onClick={() => setPage("inscribe")}
        >
          Inscribe
        </button>
        <button
          type="button"
          className={page === "profile" ? "mode active" : "mode"}
          onClick={() => {
            refreshInterrupted();
            setPage("profile");
          }}
        >
          Profile{interrupted.length ? ` (${interrupted.length})` : ""}
        </button>
        {isRegtestConnected ? (
          <button
            type="button"
            className={page === "studio" ? "mode active" : "mode"}
            onClick={() => setPage("studio")}
          >
            Studio Libraries
          </button>
        ) : null}
      </nav>

      {page === "studio" && isRegtestConnected ? (
        <section className="studio-libraries">
          <p className="field-help" style={{ marginTop: 0 }}>
            Regtest OCM Studio — recursion IDs point at local ord (:8081). Paste{" "}
            <code>studio-paste.js</code>, Run, then Download. Inscribe the downloaded{" "}
            <code>.html</code> (Brotli optional on top).
          </p>
          <iframe
            className="studio-libraries-frame"
            title="Studio Libraries (regtest)"
            src="/studio-regtest/index.html"
          />
        </section>
      ) : null}

      {page === "profile" ? (
        <section className="panel">
          <h2>Profile — deferred commits</h2>
          <p className="field-help">
            {isControlPace
              ? "Control mode downloads a .phechan.json after commit. Keep it safe, then Send reveal here (or Upload bundle). You can change reveal fee / vanity on each row — commit is already funded."
              : "Fast mode finishes reveal in one session. This list is crash recovery if reveal failed after commit — edit reveal fee/vanity if needed, then Send reveal. Switch Inscribe → Control for deferred reveal + bundles."}
          </p>
          <div className="verify-row" style={{ marginBottom: "1rem" }}>
            <input
              ref={bundleFileRef}
              type="file"
              accept=".json,.phechan.json,application/json"
              style={{ display: "none" }}
              onChange={(e) => void onUploadCommitBundle(e.target.files?.[0] || null)}
            />
            <button
              type="button"
              className="ghost"
              disabled={busy}
              onClick={() => bundleFileRef.current?.click()}
            >
              Upload bundle
            </button>
          </div>
          {!interrupted.length ? (
            <p className="muted">No deferred commits. Inscribe → commit, or upload a bundle.</p>
          ) : (
            <ul style={{ listStyle: "none", padding: 0, margin: 0 }}>
              {interrupted.map((item) => {
                const vPre = item.plan.vanityPrefix || "";
                const vSuf = item.plan.vanitySuffix || "";
                const revealFee =
                  item.plan.revealFeeRate ?? item.plan.feeRate ?? 1;
                const vanityEst = estimateVanity(vPre, vSuf);
                return (
                <li
                  key={item.commitTxid}
                  style={{
                    borderTop: "1px solid var(--line)",
                    padding: "0.85rem 0",
                  }}
                >
                  <div>
                    <code style={{ wordBreak: "break-all", fontSize: "0.8em" }}>
                      {item.commitTxid}
                    </code>
                  </div>
                  <p className="field-help" style={{ margin: "0.35rem 0" }}>
                    {item.network} · {new Date(item.savedAt).toLocaleString()}
                    {item.plan.sameSatParent
                      ? " · same-sat parent"
                      : item.plan.parentId
                        ? " · parent-child FI/FO"
                        : ""}
                    {item.plan.parentId ? ` · parent ${item.plan.parentId}` : ""}
                  </p>
                    <p className="field-help" style={{ margin: "0.25rem 0 0.5rem" }}>
                      Commit is already funded — edit <strong>reveal fee / vanity</strong> below, then
                      Send reveal. Lowering fee below the funded budget returns surplus as{" "}
                      <strong>change to the payment address that funded the commit</strong>
                      {item.plan.paymentAddress
                        ? ` (${item.plan.paymentAddress.slice(0, 12)}…)`
                        : ""}
                      . Postage stays on the child / destination; parent returns to vault.
                    </p>
                  {signReview ? (
                    <p className="field-help" style={{ color: "var(--accent, #f97316)", margin: "0.35rem 0" }}>
                      Sign review is ready below — click <strong>{signReview.buttonLabel}</strong>, then
                      approve the wallet popup (parent + commit inputs).
                    </p>
                  ) : revealingCommitTxid === item.commitTxid ? (
                    <p className="field-help" style={{ color: "var(--accent, #f97316)", margin: "0.35rem 0" }}>
                      {grindNote || "Building parent-child reveal PSBT…"} — wallet popup comes after
                      you approve the sign-review panel.
                    </p>
                  ) : null}
                  <div className="row two" style={{ marginBottom: "0.5rem" }}>
                    <div>
                      <label htmlFor={`rev-fee-${item.commitTxid.slice(0, 8)}`}>
                        Reveal fee (sat/vB)
                      </label>
                      <input
                        id={`rev-fee-${item.commitTxid.slice(0, 8)}`}
                        type="number"
                        min={0.1}
                        step="any"
                        disabled={busy}
                        value={revealFee}
                        onChange={(e) => {
                          const n = parseFeeRateInput(e.target.value, 1);
                          patchInterruptedReveal(item.commitTxid, {
                            revealFeeRate: n,
                            feeRate: n,
                          });
                        }}
                      />
                    </div>
                    <div>
                      <label>Reveal vanity</label>
                      <div className="verify-row" style={{ gap: "0.35rem" }}>
                        <input
                          aria-label="Reveal vanity prefix"
                          placeholder="prefix"
                          spellCheck={false}
                          disabled={busy}
                          value={vPre}
                          style={{ flex: 1, minWidth: 0 }}
                          onChange={(e) =>
                            patchInterruptedReveal(item.commitTxid, {
                              vanityPrefix: sanitizeVanityHex(
                                e.target.value,
                                MAX_VANITY_TOTAL - vSuf.length
                              ),
                            })
                          }
                        />
                        <span className="muted">…</span>
                        <input
                          aria-label="Reveal vanity suffix"
                          placeholder="suffix"
                          spellCheck={false}
                          disabled={busy}
                          value={vSuf}
                          style={{ flex: 1, minWidth: 0 }}
                          onChange={(e) =>
                            patchInterruptedReveal(item.commitTxid, {
                              vanitySuffix: sanitizeVanityHex(
                                e.target.value,
                                MAX_VANITY_TOTAL - vPre.length
                              ),
                            })
                          }
                        />
                        <button
                          type="button"
                          className="ghost"
                          disabled={busy || (!vPre && !vSuf)}
                          title="Clear reveal vanity"
                          onClick={() =>
                            patchInterruptedReveal(item.commitTxid, {
                              vanityPrefix: "",
                              vanitySuffix: "",
                            })
                          }
                        >
                          Clear
                        </button>
                      </div>
                      {(vPre || vSuf) && (
                        <p className="field-help" style={{ margin: "0.25rem 0 0" }}>
                          {vPre}…{vSuf} · {vanityEst.description} · ETA {vanityEst.eta}
                          {vPre.length + vSuf.length >= 6
                            ? " — 6 chars often fails (5M try cap); clear or use ≤3"
                            : ""}
                        </p>
                      )}
                    </div>
                  </div>
                  <div className="verify-row">
                    {revealingCommitTxid === item.commitTxid ? (
                      <button type="button" className="ghost" onClick={cancelReveal}>
                        Cancel grind
                      </button>
                    ) : (
                      <button
                        type="button"
                        className="primary"
                        disabled={busy || !address}
                        title={
                          !address
                            ? "Connect wallet first"
                            : busy
                              ? "Another reveal is in progress"
                              : "Build reveal PSBT (grinds vanity first if set)"
                        }
                        onClick={() => void sendRevealFromProfile(item)}
                      >
                        Send reveal
                      </button>
                    )}
                    <button
                      type="button"
                      className="ghost"
                      disabled={busy}
                      onClick={() => redownloadBundle(item)}
                    >
                      Download bundle
                    </button>
                    <button
                      type="button"
                      className="ghost"
                      disabled={busy}
                      onClick={() => {
                        removeInterruptedCommit(item.commitTxid);
                        if (pendingReveal?.commitTxid === item.commitTxid) {
                          setPendingReveal(null);
                        }
                        refreshInterrupted();
                      }}
                    >
                      Dismiss
                    </button>
                    <a
                      href={explorerTxUrl(item.network || network, item.commitTxid)}
                      target="_blank"
                      rel="noreferrer"
                      className="ghost"
                      style={{ display: "inline-flex", alignItems: "center" }}
                    >
                      Explorer
                    </a>
                  </div>
                </li>
                );
              })}
            </ul>
          )}
        </section>
      ) : null}

      {page === "inscribe" ? (
      <section className="panel">
        <h2>Inscribe</h2>
        <div className="pace-toggle" role="group" aria-label="Inscribe pace">
          <button
            type="button"
            className={inscribePace === "fast" ? "pace active" : "pace"}
            disabled={busy || Boolean(signReview)}
            onClick={() => setPace("fast")}
          >
            Fast
          </button>
          <button
            type="button"
            className={inscribePace === "control" ? "pace active" : "pace"}
            disabled={busy || Boolean(signReview)}
            onClick={() => setPace("control")}
          >
            Control
          </button>
        </div>
        <p className="field-help" style={{ marginTop: 0 }}>
          {isControlPace
            ? "Control — commit now at a low fee, download a .phechan.json bundle, reveal later (any network your wallet is on)."
            : "Fast — commit and reveal in one session (classic flow). Profile still recovers if reveal fails mid-way."}
        </p>
        <div className="mode-row">
          {(
            [
              ["text", "Text"],
              ["delegate", "Delegate"],
              ["upload", "Upload"],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              className={mode === id ? "mode active" : "mode"}
              onClick={() => setMode(id)}
            >
              {label}
            </button>
          ))}
        </div>

        {mode === "text" && (
          <>
            <label htmlFor="body">Text</label>
            <textarea id="body" value={body} onChange={(e) => setBody(e.target.value)} rows={5} />
          </>
        )}

        {mode === "delegate" && (
          <>
            <p className="hint">
              Delegate (tag 11) points at another inscription&apos;s content. No body is stored —
              indexers resolve content from the target ID (may 404 until that inscription exists).
            </p>
            <label htmlFor="delegate">Delegate inscription ID</label>
            <div className="verify-row">
              <input
                id="delegate"
                value={delegateId}
                onChange={(e) => {
                  setDelegateId(e.target.value);
                  setDelegateVerify("idle");
                  setDelegateMsg("");
                }}
                placeholder="&lt;64-hex&gt;i0"
                spellCheck={false}
              />
              <button
                type="button"
                className="ghost"
                disabled={busy || !delegateId.trim() || delegateVerify === "loading"}
                onClick={verifyDelegate}
              >
                {delegateVerify === "loading" ? "Verifying…" : "Verify"}
              </button>
            </div>
            {delegateVerify === "ok" && <div className="verify-ok">{delegateMsg}</div>}
            {delegateVerify === "error" && <div className="verify-bad">{delegateMsg}</div>}
          </>
        )}

        {mode === "upload" && (
          <>
            <label htmlFor="file">File</label>
            <input id="file" type="file" onChange={(e) => onUpload(e.target.files?.[0] || null)} />
            {uploadName && (
              <p className="hint">
                {uploadName} · {uploadType || "unknown"} · ~{((uploadB64.length * 0.75) | 0).toLocaleString()}{" "}
                bytes
              </p>
            )}
          </>
        )}

        <details
          className="fold"
          open={showOptional}
          onToggle={(e) => setShowOptional((e.target as HTMLDetailsElement).open)}
        >
          <summary>Optional — title, metadata, metaprotocol</summary>

          <label htmlFor="title">Title</label>
          <p className="field-help">
            Ordinals <strong>Properties tag 17</strong> — Attributes key 0. This is what explorers
            show as <em>title</em> (e.g.{" "}
            <a
              href="https://ordinals.com/inscription/420a3e99e9252b45f6185c1cee22294dc1671725da3fc54ce3e77480e9372a69i0"
              target="_blank"
              rel="noreferrer"
            >
              BHANG
            </a>
            ). Independent of Metadata — leave Metadata empty if you only want a title.
          </p>
          <input id="title" value={title} onChange={(e) => setTitle(e.target.value)} maxLength={200} />

          <label htmlFor="mp">Metaprotocol</label>
          <p className="field-help">
            Ordinals <strong>tag 7</strong> — free-form UTF-8 string. There is no registry; any
            identifier you choose is valid on-chain (e.g. <code>bhang</code>). Clients may ignore
            unknown values. Not a MIME type and not Metadata.
          </p>
          <input
            id="mp"
            value={metaprotocol}
            onChange={(e) => setMetaprotocol(e.target.value)}
            placeholder="optional protocol id"
            maxLength={200}
          />

          <label htmlFor="meta">Metadata</label>
          <p className="field-help">
            Ordinals <strong>tag 5</strong> — ideally CBOR. Phechan accepts UTF-8 text/JSON and
            pushes those bytes (chunked at 520). Optional and separate from Title — empty means no
            metadata tag. Do not paste private keys.
          </p>
          <textarea
            id="meta"
            value={metadata}
            onChange={(e) => setMetadata(e.target.value)}
            rows={3}
            placeholder='e.g. {"name":"Piece","attributes":[]}'
          />

          {brotliEligible && (
            <>
              <label htmlFor="br" className="check-label">
                <input
                  id="br"
                  type="checkbox"
                  checked={compressBr}
                  onChange={(e) => setCompressBr(e.target.checked)}
                />{" "}
                Brotli compress body
              </label>
              <p className="field-help">
                Ordinals <strong>content_encoding tag 9</strong> = <code>br</code>. Smaller body in
                the reveal witness → lower vsize → lower fee at your sat/vB. Clients must decompress.
              </p>
              {brotliStats && (
                <div
                  className={`brotli-stats ${compressBr && brotliStats.savedBytes > 0 ? "on" : ""}`}
                >
                  <div>
                    Raw body <strong>{brotliStats.rawBytes.toLocaleString()}</strong> B
                  </div>
                  <div>
                    Brotli <strong>{brotliStats.compressedBytes.toLocaleString()}</strong> B
                    {brotliStats.savedBytes > 0
                      ? ` (−${brotliStats.savedPct}% / −${brotliStats.savedBytes.toLocaleString()} B)`
                      : " (larger — no savings)"}
                  </div>
                  {brotliStats.savedBytes > 0 ? (
                    <div>
                      Est. fee save ≈ <strong>{brotliStats.savedFeeSats}</strong> sats at {revealFeeRate}{" "}
                      sat/vB (~{brotliStats.savedVbytes} vB witness)
                    </div>
                  ) : (
                    <div className="muted">
                      Tiny payloads often grow under Brotli (header overhead). Use Brotli for larger
                      HTML/JSON (hundreds of bytes+), not short text like this.
                    </div>
                  )}
                  {compressBr && brotliStats.savedBytes <= 0 && (
                    <div className="muted">Unchecked recommended — compression would increase size.</div>
                  )}
                  {!compressBr && brotliStats.savedBytes > 0 && (
                    <div className="muted">Tick the box to apply compression on Inscribe.</div>
                  )}
                </div>
              )}
            </>
          )}
        </details>

        <details
          className="fold"
          open={showAdvanced}
          onToggle={(e) => setShowAdvanced((e.target as HTMLDetailsElement).open)}
        >
          <summary>Advanced — parent &amp; sat targeting</summary>

          <label htmlFor="parent">Parent inscription ID</label>
          <p className="field-help">
            Verify the parent you hold. Placement is automatic from the fields below —{" "}
            <strong>no separate mode toggle</strong>:
          </p>
          <ul className="field-help" style={{ marginTop: 0 }}>
            <li>
              Parent only (or parent + a <em>different</em> sat) → child on a <strong>new sat</strong>{" "}
              (FI/FO): sign Payment at fund, Ordinals at reveal.
            </li>
            <li>
              Parent + Target sat/inscription resolving to the <strong>same UTXO</strong> → child on
              the <strong>parent sat</strong> (reinscription + provenance tag 3): parent UTXO moves
              into commit; usually one Ordinals sign (plus Payment only if that UTXO is too small for
              fees).
            </li>
          </ul>
          <div className="verify-row">
            <input
              id="parent"
              value={parentId}
              onChange={(e) => {
                setParentId(e.target.value);
                setParentVerify("idle");
                setParentMsg("");
                setParentOutpoint("");
                setParentValue(null);
                setParentAddress("");
              }}
              placeholder="abc…i0"
              spellCheck={false}
            />
            <button
              type="button"
              className="ghost"
              disabled={busy || !parentId.trim() || parentVerify === "loading"}
              onClick={verifyParent}
            >
              {parentVerify === "loading" ? "Verifying…" : "Verify"}
            </button>
          </div>
          {parentVerify === "ok" && (
            <div className="verify-ok" style={{ whiteSpace: "pre-wrap" }}>
              {parentMsg}
            </div>
          )}
          {parentVerify === "error" && <div className="verify-bad">{parentMsg}</div>}

          {parentVerify === "ok" && (
            <div
              className={sameSatParent ? "verify-ok" : parentChildDifferentSat ? "verify-ok" : "hint"}
              style={{ marginTop: "0.75rem", whiteSpace: "pre-wrap" }}
            >
              {sameSatParent
                ? "Detected: same-sat parent/child (parent UTXO = target).\nBefore Inscribe: you will sign once at fund (Ordinals; Payment only if top-up). Reveal has no second wallet sign."
                : parentChildDifferentSat
                  ? "Detected: different-sat child (FI/FO).\nBefore Inscribe: you will sign twice — Payment (fund commit), then Ordinals (spend parent at reveal)."
                  : satTarget.trim()
                    ? "Verify Target sat / inscription to confirm same-sat vs different-sat."
                    : "Optional: paste the same parent id (or its sat) under Target and Verify for same-sat child on the parent."}
            </div>
          )}

          <label htmlFor="sat">Target sat or inscription</label>
          <p className="field-help">
            For same-sat parent→child: paste the <strong>same parent id</strong> (or its sat number) and
            Verify — we match outpoints. Leave blank for different-sat FI/FO when parent is set. Or
            target any other sat you hold (reinscription / rare sat) without a parent.
          </p>
          <div className="verify-row">
            <input
              id="sat"
              value={satTarget}
              onChange={(e) => {
                setSatTarget(e.target.value);
                setSatVerify("idle");
                setSatMsg("");
                setSatReinscribe(false);
                setSatResolvedOutpoint("");
                setSatResolvedValue(null);
              }}
              placeholder="1234567890  or  abc…i0"
              spellCheck={false}
            />
            <button
              type="button"
              className="ghost"
              disabled={busy || !satTarget.trim() || satVerify === "loading"}
              onClick={verifySat}
            >
              {satVerify === "loading" ? "Verifying…" : "Verify"}
            </button>
          </div>
          {satVerify === "ok" && (
            <div className="verify-ok">
              {satMsg}
              {satReinscribe && (
                <div className="reinscribe-badge">Reinscription — sat already carries inscription(s)</div>
              )}
              {satResolvedOutpoint && (
                <div className="hint">Resolved outpoint: {satResolvedOutpoint}</div>
              )}
            </div>
          )}
          {satVerify === "error" && <div className="verify-bad">{satMsg}</div>}
        </details>

        <details
          className="fold"
          open={showTx}
          onToggle={(e) => setShowTx((e.target as HTMLDetailsElement).open)}
        >
          <summary>Transaction options — fee rate, postage, UTXO, vanity</summary>
          <label htmlFor="opr">OP_RETURN (optional)</label>
          <input id="opr" value={opReturn} onChange={(e) => setOpReturn(e.target.value)} />
          <p className="field-help">
            When set: reveal vout0 = message (0 sats), then parent/vault, child, change.
            When empty: vault stays vout0 (unchanged).
          </p>

          <label htmlFor="utxo">Funding UTXO (payment only)</label>
          <p className="field-help">
            Pick a <strong>payment</strong> UTXO (nested 2…/tb1q) big enough for commit + fee.
            This is <em>not</em> the parent inscription — parent is chosen under Advanced →
            Verify. Funding UTXO is only used when Inscribe builds the commit.
          </p>
          <p className="field-help">
            Loaded from local esplora (regtest) / mempool (other nets). Dropdown lists{" "}
            <strong>all</strong> spendable UTXOs on the <strong>payment</strong> address — no
            display cap. After a spend, change returns here as one slightly smaller UTXO (e.g.
            12.5 → ~12.4999) — it replaces the spent coin, it is not a second balance. Regtest
            only hides immature <em>coinbase</em> rewards (&lt;100 conf), not payment change.
            Auto picks the smallest that covers commit + fee.
          </p>
          <div className="verify-row">
            <select
              id="utxo"
              value={selectedUtxoKey}
              onChange={(e) => setSelectedUtxoKey(e.target.value)}
            >
              <option value="">Auto-select</option>
              {fundingUtxos.map((u) => (
                <option key={`${u.txid}:${u.vout}`} value={`${u.txid}:${u.vout}`}>
                  {u.value.toLocaleString()} sats · {u.txid.slice(0, 8)}…:{u.vout}
                  {u.confirmations != null ? ` · ${u.confirmations} conf` : ""}
                </option>
              ))}
            </select>
            <button
              type="button"
              className="ghost"
              disabled={busy || !(paymentAddress || address)}
              onClick={async () => {
                const pay = paymentAddress || address;
                setBusy(true);
                const u = await getWalletUtxos(pay, network);
                setBusy(false);
                setFundingUtxos(u.utxos);
                setSelectedUtxoKey("");
                setOk(u.ok);
                const totalSats = u.utxos.reduce((s, x) => s + x.value, 0);
                const totalBtc = (totalSats / 1e8).toFixed(8);
                setOut(
                  [
                    u.message,
                    `network: ${network}`,
                    pay && `payment: ${pay}`,
                    u.utxos.length
                      ? `spendable total: ${totalSats.toLocaleString()} sats (${totalBtc} BTC) across ${u.utxos.length} UTXO(s) — all listed in the dropdown`
                      : null,
                    ...u.utxos.map(
                      (x) =>
                        `${x.value.toLocaleString()} sats  ${x.txid.slice(0, 10)}…:${x.vout}` +
                        (x.confirmations != null ? `  (${x.confirmations} conf)` : "")
                    ),
                  ]
                    .filter(Boolean)
                    .join("\n")
                );
              }}
            >
              Load UTXOs
            </button>
          </div>

          <p className="field-help" style={{ marginTop: "0.75rem" }}>
            <strong>Commit</strong> vanity — grind funding TXID before wallet sign. Hex{" "}
            <code>0-9 a-f</code>, max {MAX_VANITY_TOTAL} chars total. Locktime is capped to the
            current {network} tip/mediantime so the tx is final (prevents{" "}
            <code>rpc -26: non-final</code>).
          </p>
          <div className="row two">
            <div>
              <label htmlFor="cvpre">
                Commit prefix ({commitVanityPrefix.length}/
                {MAX_VANITY_TOTAL - commitVanitySuffix.length})
              </label>
              <input
                id="cvpre"
                value={commitVanityPrefix}
                onChange={(e) =>
                  setCommitVanityPrefix(
                    sanitizeVanityHex(e.target.value, MAX_VANITY_TOTAL - commitVanitySuffix.length)
                  )
                }
                placeholder="420"
                spellCheck={false}
              />
            </div>
            <div>
              <label htmlFor="cvsuf">
                Commit suffix ({commitVanitySuffix.length}/
                {MAX_VANITY_TOTAL - commitVanityPrefix.length})
              </label>
              <input
                id="cvsuf"
                value={commitVanitySuffix}
                onChange={(e) =>
                  setCommitVanitySuffix(
                    sanitizeVanityHex(e.target.value, MAX_VANITY_TOTAL - commitVanityPrefix.length)
                  )
                }
                placeholder="69"
                spellCheck={false}
              />
            </div>
          </div>
          {(commitVanityPrefix || commitVanitySuffix) && (
            <div className="vanity-est">
              <span className="mono">
                {commitVanityPrefix}
                <span className="muted">xxxxx…</span>
                {commitVanitySuffix}
              </span>
              <span>
                {commitVanityDiff.description} · ~{commitVanityDiff.avgAttempts.toLocaleString()} · ETA{" "}
                {commitVanityDiff.eta}
              </span>
            </div>
          )}

          <p className="field-help" style={{ marginTop: "0.75rem" }}>
            <strong>Reveal</strong> vanity — grind after commit is funded.
          </p>
          <div className="row two">
            <div>
              <label htmlFor="vpre">
                Reveal prefix ({vanityPrefix.length}/{MAX_VANITY_TOTAL - vanitySuffix.length})
              </label>
              <input
                id="vpre"
                value={vanityPrefix}
                onChange={(e) =>
                  setVanityPrefix(
                    sanitizeVanityHex(e.target.value, MAX_VANITY_TOTAL - vanitySuffix.length)
                  )
                }
                placeholder="dead"
                spellCheck={false}
              />
            </div>
            <div>
              <label htmlFor="vsuf">
                Reveal suffix ({vanitySuffix.length}/{MAX_VANITY_TOTAL - vanityPrefix.length})
              </label>
              <input
                id="vsuf"
                value={vanitySuffix}
                onChange={(e) =>
                  setVanitySuffix(
                    sanitizeVanityHex(e.target.value, MAX_VANITY_TOTAL - vanityPrefix.length)
                  )
                }
                placeholder="cafe"
                spellCheck={false}
              />
            </div>
          </div>
          {(vanityPrefix || vanitySuffix) && (
            <div className="vanity-est">
              <span className="mono">
                {vanityPrefix}
                <span className="muted">xxxxx…</span>
                {vanitySuffix}
              </span>
              <span>
                {vanityDiff.description} · ~{vanityDiff.avgAttempts.toLocaleString()} attempts · ETA{" "}
                {vanityDiff.eta}
              </span>
            </div>
          )}

          {isControlPace ? (
            <div className="row two">
              <div>
                <label htmlFor="commit-fee">Commit fee rate (sats/vB)</label>
                <p className="field-help">
                  Pays the <em>funding</em> transaction only. Use a low rate when you are not in a hurry
                  to confirm the commit.
                </p>
                <input
                  id="commit-fee"
                  value={commitFeeRate}
                  onChange={(e) => setCommitFeeRate(e.target.value)}
                  inputMode="decimal"
                />
              </div>
              <div>
                <label htmlFor="reveal-fee">Reveal fee rate (sats/vB)</label>
                <p className="field-help">
                  Sizes how many sats go into the commit output (postage + reveal fee). Use a higher
                  rate if you expect to reveal later under congestion (e.g. a palindromic block).
                </p>
                <input
                  id="reveal-fee"
                  value={revealFeeRate}
                  onChange={(e) => setRevealFeeRate(e.target.value)}
                  inputMode="decimal"
                />
              </div>
            </div>
          ) : (
            <div className="row two">
              <div>
                <label htmlFor="fee">Fee rate (sats/vB)</label>
                <p className="field-help">
                  Applies to <em>both</em> commit funding and reveal sizing (fast mode).
                </p>
                <input
                  id="fee"
                  value={revealFeeRate}
                  onChange={(e) => {
                    const v = e.target.value;
                    setRevealFeeRate(v);
                    setCommitFeeRate(v);
                  }}
                  inputMode="decimal"
                />
              </div>
              <div>
                <label htmlFor="postage-fast">Postage (sats)</label>
                <p className="field-help">
                  {postageLocked
                    ? "Locked to the target inscription UTXO value (reinscription)."
                    : "Inscription output value (padding). Examples: 330 / 545 / 546 (≥ ~330 dust)."}
                </p>
                <input
                  id="postage-fast"
                  value={postage}
                  onChange={(e) => setPostage(e.target.value)}
                  inputMode="numeric"
                  placeholder="546"
                  disabled={postageLocked}
                  readOnly={postageLocked}
                />
              </div>
            </div>
          )}
          {isControlPace ? (
            <div className="row two">
              <div>
                <label htmlFor="postage">Postage (sats)</label>
                <p className="field-help">
                  {postageLocked
                    ? "Locked to the target inscription UTXO value (reinscription)."
                    : "Inscription output value (padding). Examples: 330 / 545 / 546 (≥ ~330 dust)."}
                </p>
                <input
                  id="postage"
                  value={postage}
                  onChange={(e) => setPostage(e.target.value)}
                  inputMode="numeric"
                  placeholder="546"
                  disabled={postageLocked}
                  readOnly={postageLocked}
                />
              </div>
              <div>
                <p className="field-help" style={{ marginTop: "1.6rem" }}>
                  Fractional rates (e.g. 0.69) are fine — broadcast via public Esplora like sort-utxo /
                  runes-etch.
                </p>
              </div>
            </div>
          ) : (
            <p className="field-help">
              Fractional rates (e.g. 0.69) are fine — broadcast via public Esplora like sort-utxo /
              runes-etch.
            </p>
          )}
        </details>

        <div className="actions">
          <button
            type="button"
            className="ghost"
            disabled={!contentPreview}
            onClick={() => setShowPreview((v) => !v)}
            title={
              contentPreview
                ? "Toggle content preview (works during grind / sign / wait)"
                : "Add text or upload a file to preview"
            }
          >
            {showPreview ? "Hide preview" : "Preview"}
          </button>
          <button
            type="button"
            className="primary"
            disabled={busy || !address || Boolean(signReview)}
            onClick={() => void runInscribe()}
          >
            Prepare Inscribe
          </button>
        </div>
        <div className="network-cost" aria-live="polite">
          {networkCost.status === "idle" && (
            <p className="network-cost-line muted">{networkCost.detail || "Network fees"}</p>
          )}
          {networkCost.status === "loading" && (
            <p className="network-cost-line">Network fees — estimating…</p>
          )}
          {networkCost.status === "error" && (
            <p className="network-cost-line bad">Network fees — {networkCost.detail}</p>
          )}
          {networkCost.status === "ok" && (
            <>
              <p className="network-cost-line">
                Network fees ≈{" "}
                <strong>{networkCost.networkFeeSats?.toLocaleString() ?? "?"} sats</strong>
                <span className="muted">
                  {" "}
                  (reveal {networkCost.revealFeeSats?.toLocaleString() ?? "?"}
                  {networkCost.commitFeeSats != null
                    ? ` + commit ≈ ${networkCost.commitFeeSats.toLocaleString()}`
                    : ""}
                  )
                </span>
              </p>
              <p className="network-cost-line muted">
                Postage {networkCost.postageSats?.toLocaleString() ?? "?"} sats — returned with the
                inscription (not a miner fee). Fund commit with{" "}
                {networkCost.fundCommitSats?.toLocaleString() ?? "?"} sats.
              </p>
            </>
          )}
        </div>

        <div className="custody-panel">
          <h3 className="custody-heading">Self Custody</h3>
          <p>
            Inscription tapscript is generated using your wallet&apos;s public key. This has added
            benefits.
          </p>
          <p>Reveal transaction is signed with your public key, adding another layer of provenance.</p>
          <p>You can recover funds from the tapscript address with a key-path spend.</p>
          <h3 className="custody-heading">What a sign means</h3>
          <p>
            <strong>Sign #1 (Payment)</strong> — authorize the funding tx that creates the commit
            output. We broadcast that commit immediately.
          </p>
          <p>
            <strong>Sign #2 (Ordinals)</strong> — authorize the reveal that spends the commit (and
            parent, for FI/FO). We broadcast that reveal immediately.
          </p>
          <p>
            Control only defers <em>when</em> you do sign #2 — it does not hold a signed tx
            unbroadcast. We never keep a wallet-signed transaction offline for later packaging.
          </p>
        </div>

        {showPreview && contentPreview && (
          <div className="content-preview">
            <p className="field-help">Content preview (how the payload renders — not a PSBT).</p>
            {contentPreview.kind === "html" ? (
              <iframe
                title="Inscription preview"
                className="preview-frame"
                sandbox=""
                srcDoc={contentPreview.text}
              />
            ) : (
              <pre className="preview-text">{contentPreview.text}</pre>
            )}
          </div>
        )}

        {flowPhase !== "idle" && (
          <div className={`flow-status ${flowPhase}`}>
            <div className="flow-steps">
              <span
                className={
                  flowPhase === "preparing"
                    ? "on"
                    : ["funding", "review", "grinding", "committed", "done"].includes(flowPhase)
                      ? "done"
                      : ""
                }
              >
                1 Prepare
              </span>
              <span
                className={
                  flowPhase === "funding" || flowPhase === "review"
                    ? "on"
                    : ["grinding", "committed", "done"].includes(flowPhase)
                      ? "done"
                      : ""
                }
              >
                {isControlPace ? "2 Commit" : "2 Review + sign"}
              </span>
              <span
                className={
                  flowPhase === "grinding"
                    ? "on"
                    : flowPhase === "committed"
                      ? "on"
                      : flowPhase === "done"
                        ? "done"
                        : ""
                }
              >
                {isControlPace ? "3 Reveal (when ready)" : "3 Reveal"}
              </span>
            </div>
            {grindNote && <p className="grind-note">{grindNote}</p>}
            {flowPhase === "committed" && isControlPace && pendingReveal && (
              <div className="actions" style={{ marginTop: "0.75rem" }}>
                <button
                  type="button"
                  className="primary"
                  disabled={busy || !address}
                  onClick={() => void sendRevealFromProfile(pendingReveal)}
                >
                  Reveal now
                </button>
                <button
                  type="button"
                  className="ghost"
                  disabled={busy}
                  onClick={() => redownloadBundle(pendingReveal)}
                >
                  Re-download bundle
                </button>
                <button
                  type="button"
                  className="ghost"
                  disabled={busy}
                  onClick={() => {
                    refreshInterrupted();
                    setPage("profile");
                  }}
                >
                  Open Profile
                </button>
              </div>
            )}
          </div>
        )}

        {resultTx?.commitTxid && !resultTx.revealTxid && isControlPace && (
          <div className="result-card">
            <p>
              <strong>Commit</strong>{" "}
              <a href={explorerTxUrl(network, resultTx.commitTxid)} target="_blank" rel="noreferrer">
                {resultTx.commitTxid}
              </a>
            </p>
            <p className="field-help">
              Reveal deferred — keep the downloaded <code>.phechan.json</code> until you send reveal.
            </p>
          </div>
        )}

        {resultTx?.revealTxid && (
          <div className="result-card">
            <p>
              <strong>Reveal</strong>{" "}
              <a href={explorerTxUrl(network, resultTx.revealTxid)} target="_blank" rel="noreferrer">
                {resultTx.revealTxid.slice(0, 16)}…
              </a>
            </p>
            {resultTx.inscriptionId && (
              <p className="mono">inscription {resultTx.inscriptionId}</p>
            )}
            {resultTx.commitTxid && (
              <p className="muted">
                commit{" "}
                <a href={explorerTxUrl(network, resultTx.commitTxid)} target="_blank" rel="noreferrer">
                  {resultTx.commitTxid.slice(0, 12)}…
                </a>
              </p>
            )}
          </div>
        )}
      </section>
      ) : null}

      {signReview && (
        <div className={`sign-review sign-review-global${isMainnetNet ? " mainnet" : ""}`}>
          <h3>{signReview.headline}</h3>
          <p className="field-help" style={{ marginTop: 0 }}>
            Parent-child reveal needs your wallet to sign after you approve here — this panel shows
            on Profile and Inscribe.
          </p>
          <ul className="sign-review-steps">
            {signReview.steps.map((s) => (
              <li key={s}>{s}</li>
            ))}
          </ul>
          {signReview.warnings.length > 0 && (
            <ul className="sign-review-warn">
              {signReview.warnings.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          )}
          <div className="sign-review-io">
            <div>
              <h4>Inputs</h4>
              {signReview.inputs.length ? (
                <ul>
                  {signReview.inputs.map((line, i) => (
                    <li key={`in-${i}`} className="mono">
                      {line}
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="muted">Wallet will show inputs.</p>
              )}
            </div>
            <div>
              <h4>Outputs</h4>
              {signReview.outputs.length ? (
                <ul>
                  {signReview.outputs.map((line, i) => (
                    <li key={`out-${i}`} className="mono">
                      {line}
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="muted">Wallet will show outputs.</p>
              )}
            </div>
          </div>
          {signReview.meta.length > 0 && (
            <p className="muted mono">{signReview.meta.join(" · ")}</p>
          )}
          <div className="actions">
            <button
              type="button"
              className="primary"
              disabled={busy}
              onClick={approveSignReview}
            >
              {signReview.buttonLabel}
            </button>
            <button type="button" className="ghost" disabled={busy} onClick={cancelSignReview}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {out && (
        <pre className={`out ${ok === true ? "ok" : ok === false ? "bad" : ""}`}>{out}</pre>
      )}
      <p className="health">{health}</p>
    </div>
  );
}
