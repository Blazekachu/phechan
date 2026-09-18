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
type FlowPhase = "idle" | "preparing" | "funding" | "review" | "grinding" | "done" | "error";
type AppPage = "inscribe" | "profile";

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
  return /cancel|reject|denied|4001/i.test(msg);
}

function isTransientError(msg: string) {
  return /timeout|network|fetch|ECONN|temporar|503|502|429|esplora/i.test(msg);
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

  const [showTx, setShowTx] = useState(false);
  const [opReturn, setOpReturn] = useState("");
  const [vanityPrefix, setVanityPrefix] = useState("");
  const [vanitySuffix, setVanitySuffix] = useState("");
  const [commitVanityPrefix, setCommitVanityPrefix] = useState("");
  const [commitVanitySuffix, setCommitVanitySuffix] = useState("");
  const [feeRate, setFeeRate] = useState("1");
  const [postage, setPostage] = useState("546");
  const [fundingUtxos, setFundingUtxos] = useState<
    { txid: string; vout: number; value: number; scriptPubKey?: string; address?: string }[]
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
  /** Crash-recovery only — Inscribe keeps commit+reveal in one session */
  const [interrupted, setInterrupted] = useState<InterruptedCommit[]>(() =>
    listInterruptedCommits()
  );
  const [page, setPage] = useState<AppPage>("inscribe");
  const [mainnetUnlocked, setMainnetUnlocked] = useState(false);
  const [signReview, setSignReview] = useState<SignReview | null>(null);
  const signReviewResolver = useRef<((go: boolean) => void) | null>(null);

  const refreshInterrupted = useCallback(() => {
    setInterrupted(listInterruptedCommits());
  }, []);

  const isMainnetNet =
    network.toLowerCase() === "mainnet" || network.toLowerCase() === "bitcoin";
  const MAINNET_PHRASE = "BROADCAST MAINNET";

  /** Attached after user reviews disclosure — no typing. */
  const mainnetConfirm = useCallback(() => {
    if (!isMainnetNet) return undefined;
    return mainnetUnlocked ? MAINNET_PHRASE : undefined;
  }, [isMainnetNet, mainnetUnlocked, MAINNET_PHRASE]);

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
        setMainnetUnlocked(Boolean(h.mainnetUnlocked));
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
            feeRate: Number(feeRate) || 1,
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
  }, [brotliEligible, mode, body, uploadB64, feeRate]);

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
      feeRate: Number(feeRate) || 1,
      postage: Number(postage) || 546,
      destination: address || undefined,
      paymentAddress: paymentAddress || address || undefined,
      confirm: isMainnetNet && mainnetUnlocked ? MAINNET_PHRASE : undefined,
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
    feeRate,
    postage,
    address,
    paymentAddress,
    isMainnetNet,
    mainnetUnlocked,
    MAINNET_PHRASE,
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
      planOverride?: InscribePlan
    ) => {
      const activePlan = planOverride || plan;
      const sameSat = Boolean(activePlan.sameSatParent);
      const differentSat = Boolean(
        activePlan.parentId && activePlan.parentOutpoint && !sameSat
      );

      if (!address) {
        throw new Error("Connect wallet first");
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
        const nowAddr = String(prep.commit_address || "");
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
        ? `Grinding reveal TXID ${activePlan.vanityPrefix || ""}…${activePlan.vanitySuffix || ""}`
        : "Building & broadcasting reveal…";
      setGrindNote(grindMsg);
      setOut(`Commit funded: ${commitTxid}\n${grindMsg}`);

      let rev: Awaited<ReturnType<typeof api.revealInscription>> | null = null;
      for (let attempt = 0; attempt < 5; attempt++) {
        rev = await api.revealInscription({
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
        });
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

      const parentChildPsbt = Boolean(differentSat && rev.psbt_base64);
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

      if (parentChildPsbt) {
        const inspected = await api.inspectPsbt({
          base64: String(rev.psbt_base64),
          network: activePlan.network || network,
        });
        const go = await requestSignReview({
          kind: "reveal",
          headline: "Review reveal (parent spend) before wallet sign",
          steps: [
            `Commit already broadcast: ${commitTxid}`,
            "This PSBT spends the parent UTXO (vin0) + commit (vin1).",
            String(rev.parent_lands || "Parent returns on vout0 (vault)."),
            String(rev.child_lands || "Child lands on vout1 (postage)."),
            "Xverse will ask you to sign the Ordinals (parent) input.",
            "After you approve here, the wallet popup opens — then we broadcast via Esplora.",
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
          buttonLabel: "Sign reveal in wallet",
        });
        if (!go) {
          throw new Error(
            `Cancelled before reveal sign.\nCommit unspent: ${commitTxid}\nOpen Profile → Send reveal to finish later.`
          );
        }
        setSignReview(null);

        let signedPsbt = "";
        for (let round = 1; ; round++) {
          setOut(
            [
              `Commit funded: ${commitTxid}`,
              String(rev.parent_lands || ""),
              String(rev.child_lands || ""),
              round === 1
                ? "Sign parent input in wallet (ordinals) to finish inscription…"
                : `Wallet cancelled — prompt ${round}: approve Ordinals sign to finish (commit already broadcast).`,
            ]
              .filter(Boolean)
              .join("\n")
          );
          const signedReveal = await signPsbtWithWallet(String(rev.psbt_base64), {
            broadcast: false,
            signInputs: { [address]: [0] },
          });
          if (signedReveal.ok && signedReveal.psbt) {
            signedPsbt = signedReveal.psbt;
            break;
          }
          const msg = signedReveal.message || "Wallet did not sign parent input";
          if (isWalletCancel(msg)) {
            continue;
          }
          throw new Error(`${msg}\n\nCommit unspent: ${commitTxid}`);
        }

        setOut("Parent signed — broadcasting reveal via Esplora…");
        let fin: Awaited<ReturnType<typeof api.finalizeFundingPsbt>> | null = null;
        for (let attempt = 0; attempt < 5; attempt++) {
          fin = await api.finalizeFundingPsbt({
            base64: signedPsbt,
            network: activePlan.network || network,
            broadcast: true,
            confirm:
              activePlan.confirm ||
              mainnetConfirm() ||
              undefined,
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
              ? `parent returned → vout0 ${activePlan.destination || address} (${activePlan.parentValue} sats)`
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
      mainnetConfirm,
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
    if (isMainnetNet && !mainnetUnlocked) {
      setOk(false);
      setOut(
        "Mainnet locked. Restart the local API with PHECHAN_ALLOW_MAINNET_BROADCAST=1, then review the disclosure and Sign."
      );
      setFlowPhase("error");
      return;
    }
    if (mode === "delegate" && !delegateId.trim()) {
      setOk(false);
      setOut("Delegate ID required.");
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
      if (prep.error) {
        throw new Error(String(prep.error));
      }
      const commitAddress = String(prep.commit_address || "");
      const commitSats = Number(prep.commit_sats || 0);
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

      const feeR = Number(feeRate) || 1;
      const [carrierTxid, carrierVoutStr] = parentOutpoint
        ? parentOutpoint.split(":")
        : ["", ""];
      const carrierVout = Number(carrierVoutStr);
      const carrierValueSats = parentValue ?? 0;

      // Same-sat: parent UTXO is vin0. Payment top-up only if it cannot cover commit+fee.
      let needPaymentTopUp = true;
      let utxo:
        | { txid: string; vout: number; value: number; scriptPubKey?: string; address?: string }
        | undefined;

      if (sameSatParent) {
        const roughFundVb = 120; // p2tr in + commit + change
        const roughFee = Math.max(1, Math.round(roughFundVb * feeR));
        needPaymentTopUp = carrierValueSats < commitSats + roughFee;
        setOut(
          needPaymentTopUp
            ? `Same-sat parent: carrier ${parentOutpoint} (${carrierValueSats} sats) + payment top-up → commit ${commitSats}`
            : `Same-sat parent: funding commit from parent UTXO ${parentOutpoint} (${carrierValueSats} sats) alone`
        );
        if (needPaymentTopUp) {
          let utxos = fundingUtxos;
          if (!utxos.length) {
            const u = await getWalletUtxos(payAddr, network);
            if (!u.ok || !u.utxos.length) {
              throw new Error(
                u.message ||
                  `Parent UTXO alone is too small (${carrierValueSats} < ${commitSats}+fee). Need a payment UTXO on ${payAddr}.`
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
              `Parent ${carrierValueSats} sats + no payment UTXO large enough for top-up (~${needPay} sats)`
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
        changeAddress: sameSatParent ? address : payAddr,
        ...(sameSatParent
          ? {
              carrierTxid,
              carrierVout,
              carrierValue: carrierValueSats,
              carrierAddress: parentAddress || address,
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

      const signInputs: Record<string, number[]> = sameSatParent
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
            ? `Same-sat child on parent ${parentId.trim()} — parent UTXO funds commit; one wallet sign; reveal has no second sign.`
            : `Different-sat child of ${parentId.trim()} — sign #1 funds commit (Payment); after broadcast, sign #2 spends parent (Ordinals).`
          : "Standalone inscription — one funding sign, then automatic reveal.",
        `Commit output: ${commitSats} sats → ${commitAddress}`,
        `Fee rate ${feeR} sat/vB · postage ${postage} sats`,
        sameSatParent
          ? needPaymentTopUp
            ? "Wallet will sign Ordinals (vin0) + Payment (vin1) in one prompt."
            : "Wallet will sign Ordinals (parent as vin0) only."
          : "Wallet will sign Payment funding input.",
        "Nothing is broadcast until you approve below and then approve in Xverse.",
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
        buttonLabel: parentChildDifferentSat
          ? "Sign funding & continue"
          : "Sign & Inscribe",
      });
      if (!go) {
        setOk(null);
        return;
      }
      setSignReview(null);

      setOut(
        (sameSatParent
          ? needPaymentTopUp
            ? "Sign once: Ordinals (parent) + Payment (fee) in this wallet prompt…\n"
            : "Sign once: Ordinals — parent UTXO moves into commit (reveal needs no second sign)…\n"
          : parentChildDifferentSat
            ? "Sign #1 of 2: Payment funds commit (Ordinals signs parent at reveal)…\n"
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

      let commitTxid = signedTxid;
      if (!commitTxid && signedPsbt) {
        setOut("Wallet signed — broadcasting funding via Esplora…");
        const fin = await api.finalizeFundingPsbt({
          base64: signedPsbt,
          network,
          broadcast: true,
          paymentPublicKey: paymentPublicKey || undefined,
          confirm: mainnetConfirm(),
        });
        const broadcasted = String(fin.broadcast_txid || "");
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
      }
      if (!commitTxid) {
        throw new Error(
          "Wallet signed but returned no PSBT/txid — cannot broadcast funding"
        );
      }

      // Commit is on-chain — keep a snapshot so Profile can finish if this tab dies.
      upsertInterruptedCommit({
        commitTxid,
        commitAddress,
        network,
        savedAt: Date.now(),
        plan: snapshotPlan({
          ...plan,
          destination: address,
          vaultAddress: address,
          parentAddress: parentAddress || address,
          ordinalsPublicKey: ordinalsPublicKey || undefined,
          sameSatParent: sameSatParent || undefined,
        }),
      });
      refreshInterrupted();

      setOut(
        sameSatParent
          ? `Commit broadcast: ${commitTxid}\nReveal — parent already in commit…`
          : `Commit broadcast: ${commitTxid}\nFinishing reveal (same Inscribe flow)…`
      );
      try {
        await runRevealFromCommit(commitTxid, commitAddress);
      } catch (revealErr) {
        // Still interrupted in store — Profile can finish. Stay explicit.
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
    feeRate,
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
    runRevealFromCommit,
    refreshInterrupted,
    isMainnetNet,
    mainnetUnlocked,
    mainnetConfirm,
    requestSignReview,
  ]);

  async function sendRevealFromProfile(item: InterruptedCommit) {
    if (!address) {
      setOk(false);
      setOut("Connect wallet first");
      return;
    }
    const itemNet = String(item.plan.network || network).toLowerCase();
    const itemMainnet = itemNet === "mainnet" || itemNet === "bitcoin";
    if (itemMainnet && !mainnetUnlocked) {
      setOk(false);
      setOut(
        "Mainnet locked. Restart the local API with PHECHAN_ALLOW_MAINNET_BROADCAST=1."
      );
      return;
    }
    setBusy(true);
    setOk(null);
    setPage("inscribe");
    try {
      const p: InscribePlan = {
        ...item.plan,
        destination: address,
        vaultAddress: address,
        ordinalsPublicKey: ordinalsPublicKey || item.plan.ordinalsPublicKey,
        confirm: itemMainnet ? MAINNET_PHRASE : item.plan.confirm,
      };
      await runRevealFromCommit(item.commitTxid, item.commitAddress, p);
    } catch (e) {
      setFlowPhase("error");
      setOk(false);
      setOut(String(e));
      setPage("profile");
      refreshInterrupted();
    } finally {
      setBusy(false);
      setGrindNote("");
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
    const r = await api.verifySatTarget(satTarget.trim(), address, network);
    if (r.ok && r.owned) {
      setSatVerify("ok");
      setSatMsg(r.detail || "Verified");
      setSatReinscribe(Boolean(r.reinscribe));
      if (r.txid != null && r.vout != null) setSatResolvedOutpoint(`${r.txid}:${r.vout}`);
    } else {
      setSatVerify("error");
      setSatMsg(r.error || r.detail || "Not verified");
    }
  }

  return (
    <div className="app">
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
      </nav>

      {page === "profile" ? (
        <section className="panel">
          <h2>Profile — interrupted commits</h2>
          <p className="field-help">
            Prepare Inscribe shows txn I/O for review before any Xverse popup, then finishes
            commit+reveal in one go. This list is only if the tab died or reveal still failed
            after retries — <strong>Send reveal</strong> reviews/signs/broadcasts without funding again.
          </p>
          {!interrupted.length ? (
            <p className="muted">No interrupted commits.</p>
          ) : (
            <ul style={{ listStyle: "none", padding: 0, margin: 0 }}>
              {interrupted.map((item) => (
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
                  <div className="verify-row">
                    <button
                      type="button"
                      className="primary"
                      disabled={busy || !address}
                      onClick={() => void sendRevealFromProfile(item)}
                    >
                      Send reveal
                    </button>
                    <button
                      type="button"
                      className="ghost"
                      disabled={busy}
                      onClick={() => {
                        removeInterruptedCommit(item.commitTxid);
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
              ))}
            </ul>
          )}
        </section>
      ) : null}

      {page === "inscribe" ? (
      <section className="panel">
        <h2>Inscribe</h2>
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
                      Est. fee save ≈ <strong>{brotliStats.savedFeeSats}</strong> sats at {feeRate}{" "}
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

          <label htmlFor="utxo">Funding UTXO (payment only)</label>
          <p className="field-help">
            Pick a <strong>payment</strong> UTXO (nested 2…/tb1q) big enough for commit + fee.
            This is <em>not</em> the parent inscription — parent is chosen under Advanced →
            Verify. Funding UTXO is only used when Inscribe builds the commit.
          </p>
          <p className="field-help">
            Loaded from mempool (same pattern as sort-utxo). Auto picks smallest that covers commit +
            fee if you leave Auto-select.
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
                setOut(
                  [
                    u.message,
                    `network: ${network}`,
                    pay && `payment: ${pay}`,
                    ...u.utxos.slice(0, 8).map(
                      (x) => `${x.value} sats  ${x.txid.slice(0, 10)}…:${x.vout}`
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

          <div className="row two">
            <div>
              <label htmlFor="fee">Fee rate (sats/vB)</label>
              <p className="field-help">
                Applies to <em>both</em> commit funding PSBT and reveal. Fractional rates
                (e.g. 0.69) are fine — we broadcast via public Esplora like sort-utxo / runes-etch
                (local bitcoind minrelay is often 1 sat/vB and is only a fallback).
              </p>
              <input
                id="fee"
                value={feeRate}
                onChange={(e) => setFeeRate(e.target.value)}
                inputMode="decimal"
              />
            </div>
            <div>
              <label htmlFor="postage">Postage (sats)</label>
              <p className="field-help">
                Inscription output value (padding). Examples: 330 / 545 / 546 (≥ ~330 dust).
              </p>
              <input
                id="postage"
                value={postage}
                onChange={(e) => setPostage(e.target.value)}
                inputMode="numeric"
                placeholder="546"
              />
            </div>
          </div>
        </details>

        <div className="actions">
          <button
            type="button"
            className="ghost"
            disabled={busy || !contentPreview}
            onClick={() => setShowPreview((v) => !v)}
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

        {signReview && (
          <div className={`sign-review${isMainnetNet ? " mainnet" : ""}`}>
            <h3>{signReview.headline}</h3>
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
              <button
                type="button"
                className="ghost"
                disabled={busy}
                onClick={cancelSignReview}
              >
                Cancel
              </button>
            </div>
          </div>
        )}

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
              <span className={flowPhase === "preparing" ? "on" : ["funding", "review", "grinding", "done"].includes(flowPhase) ? "done" : ""}>
                1 Prepare
              </span>
              <span className={flowPhase === "funding" || flowPhase === "review" ? "on" : ["grinding", "done"].includes(flowPhase) ? "done" : ""}>
                2 Review + sign
              </span>
              <span className={flowPhase === "grinding" ? "on" : flowPhase === "done" ? "done" : ""}>
                3 Reveal
              </span>
            </div>
            {grindNote && <p className="grind-note">{grindNote}</p>}
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

      {out && (
        <pre className={`out ${ok === true ? "ok" : ok === false ? "bad" : ""}`}>{out}</pre>
      )}
      <p className="health">
        {health}
        {mainnetUnlocked ? " · mainnet unlocked (API)" : ""}
      </p>
    </div>
  );
}
