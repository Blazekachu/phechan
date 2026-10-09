/**
 * Phechan local API — binds 127.0.0.1 only.
 * Shells out to phechan CLI. Never accepts private keys.
 */
import http from "node:http";
import fs from "node:fs";
import os from "node:os";
import crypto from "node:crypto";
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import zlib from "node:zlib";
import { createServer as createViteServer } from "vite";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");
const REPO = path.resolve(ROOT, "../..");
const HOST = "127.0.0.1";
const API_PORT = Number(process.env.PHECHAN_API_PORT || 8787);
const UI_PORT = Number(process.env.PHECHAN_UI_PORT || 5173);
const ORD_URL = (process.env.PHECHAN_ORD_URL || "http://127.0.0.1:8081").replace(/\/+$/, "");

/** In-memory preview HTML for Studio Libraries (regtest) Run → /preview */
let studioPreviewHtml =
  "<!DOCTYPE html><html><body><p>No Studio preview yet</p></body></html>";

let studioSimBlock = { height: 534, enabled: true };

function simulateStudioBlock(height) {
  const h = String(height);
  const hash = crypto.createHash("sha256").update("block:" + h).digest("hex");
  const prevHash = crypto
    .createHash("sha256")
    .update("block:" + (height - 1))
    .digest("hex");
  const merkle = crypto.createHash("sha256").update("merkle:" + h).digest("hex");
  return {
    id: hash,
    height,
    version: 536870912,
    timestamp: 1700000000 + height * 600,
    bits: 386089497,
    nonce: parseInt(hash.slice(0, 8), 16),
    difficulty: 95672703408661,
    merkle_root: merkle,
    previousblockhash: prevHash,
    tx_count: 2000,
    size: 1500000,
    weight: 3993000,
    fee_range: [5, 15, 25, 40, 80],
  };
}

function json(res, status, body) {
  const data = JSON.stringify(body);
  res.writeHead(status, {
    "Content-Type": "application/json; charset=utf-8",
    "Cache-Control": "no-store",
  });
  res.end(data);
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      const raw = Buffer.concat(chunks).toString("utf8");
      if (!raw) return resolve({});
      try {
        resolve(JSON.parse(raw));
      } catch (e) {
        reject(e);
      }
    });
    req.on("error", reject);
  });
}

function readBitcoinConfRpc(confPath) {
  try {
    const text = fs.readFileSync(confPath, "utf8");
    let user = "";
    let pass = "";
    for (const line of text.split(/\r?\n/)) {
      const u = line.match(/^\s*rpcuser\s*=\s*(.+)$/i);
      const p = line.match(/^\s*rpcpassword\s*=\s*(.+)$/i);
      if (u) user = u[1].trim();
      if (p) pass = p[1].trim();
    }
    if (user && pass) return { user, pass };
  } catch {
    /* missing conf */
  }
  return null;
}

/** Point CLI at the matching local bitcoind when one exists.
 * Mainnet/testnet: do NOT default to regtest :18444 — vanity/tip use Esplora.
 */
function rpcEnvForNetwork(network) {
  if (process.env.PHECHAN_RPC_URL) return {};
  const n = String(network || "").toLowerCase();
  if (n === "signet") {
    const creds =
      readBitcoinConfRpc("F:\\bitcoin-signet\\bitcoin.conf") ||
      readBitcoinConfRpc(path.join("F:", "bitcoin-signet", "bitcoin.conf"));
    if (!creds) {
      return {
        PHECHAN_RPC_URL: "http://127.0.0.1:38332",
        PHECHAN_RPC_USER: process.env.PHECHAN_RPC_USER || "ord",
      };
    }
    return {
      PHECHAN_RPC_URL: "http://127.0.0.1:38332",
      PHECHAN_RPC_USER: creds.user,
      PHECHAN_RPC_PASS: creds.pass,
    };
  }
  if (n === "regtest" || !n) {
    return {}; // crate defaults (:18444 / phechan_plain)
  }
  // mainnet / testnet — no local RPC; CLI vanity tip uses Esplora for wallet network
  return {};
}

function runPhechan(args, { timeoutMs = 120_000, network } = {}) {
  const bin = process.env.PHECHAN_BIN;
  const mingw = process.env.PHECHAN_MINGW_BIN || "F:\\Users\\akhil\\Main\\tools\\mingw64\\bin";
  // Infer network from CLI args when caller did not pass it
  let net = network;
  if (!net) {
    const i = args.indexOf("--network");
    if (i >= 0 && args[i + 1]) net = args[i + 1];
  }
  const env = {
    ...process.env,
    ...rpcEnvForNetwork(net),
    PATH: `${mingw};${process.env.PATH || ""}`,
  };

  return new Promise((resolve) => {
    let child;
    if (bin) {
      child = spawn(bin, args, { env, windowsHide: true });
    } else {
      const cargo = process.env.CARGO || `${process.env.USERPROFILE}\\.cargo\\bin\\cargo.exe`;
      child = spawn(
        cargo,
        ["run", "-q", "-p", "phechan-cli", "--manifest-path", path.join(REPO, "Cargo.toml"), "--", ...args],
        { env, windowsHide: true, cwd: REPO }
      );
    }

    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill();
      resolve({ code: 124, stdout, stderr: stderr + "\n[timeout]" });
    }, timeoutMs);

    child.stdout.on("data", (d) => {
      stdout += d.toString();
    });
    child.stderr.on("data", (d) => {
      stderr += d.toString();
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({ code: code ?? 1, stdout, stderr });
    });
  });
}

function parseCliLines(stdout) {
  const lines = {};
  const raw = [];
  for (const line of stdout.split(/\r?\n/)) {
    if (!line.trim()) continue;
    raw.push(line);
    const idx = line.indexOf(":");
    if (idx > 0) {
      const key = line.slice(0, idx).trim();
      const val = line.slice(idx + 1).trim();
      if (lines[key] === undefined) {
        lines[key] = val;
      } else if (Array.isArray(lines[key])) {
        // Multi-value keys (note, validation_error) — skip exact dupes
        if (!lines[key].includes(val)) lines[key].push(val);
      } else if (lines[key] !== val) {
        lines[key] = [lines[key], val];
      }
      // Identical duplicate (e.g. commit_sats printed twice) — keep scalar
    }
  }
  return { fields: lines, raw };
}

function isMainnet(network) {
  const n = String(network || "").toLowerCase();
  return n === "mainnet" || n === "bitcoin";
}

/** Legacy no-op — network is taken from the connected wallet; no env/confirm gate. */
function assertMainnetGate(_body, _network) {}

/**
 * Mainnet self-custody: inscription tapscript must use the connected wallet pubkey.
 * Phechan never holds that private key — no steal surface via keystore / server keys.
 */
function requireWalletCustodyPubkey(plan, network) {
  if (!isMainnet(network)) return;
  const pk = String(plan?.ordinalsPublicKey || "").trim();
  if (!/^[0-9a-fA-F]{64}$/.test(pk) && !/^[0-9a-fA-F]{66}$/.test(pk)) {
    throw new Error(
      "mainnet self-custody requires ordinalsPublicKey from the connected wallet (no server/keystore spend key)"
    );
  }
}

function appendConfirmArg(args, body, network) {
  if (body.confirm) {
    args.push("--confirm", String(body.confirm).trim());
  }
  return args;
}

function resolveRpcCreds(network) {
  const overlay = rpcEnvForNetwork(network);
  const url =
    overlay.PHECHAN_RPC_URL ||
    process.env.PHECHAN_RPC_URL ||
    (String(network || "").toLowerCase() === "regtest" || !network
      ? "http://127.0.0.1:18444"
      : "");
  const user =
    overlay.PHECHAN_RPC_USER || process.env.PHECHAN_RPC_USER || "ord";
  const pass =
    overlay.PHECHAN_RPC_PASS ||
    process.env.PHECHAN_RPC_PASS ||
    "regtest-local-dev";
  if (!url) return null;
  // submitpackage is node-wide; strip /wallet/... path if present
  const base = String(url).replace(/\/wallet\/[^/]+\/?$/, "");
  return { url: base, user, pass };
}

async function bitcoindRpcCall(network, method, params) {
  const creds = resolveRpcCreds(network);
  if (!creds) throw new Error("no bitcoind RPC configured for this network");
  const auth = Buffer.from(`${creds.user}:${creds.pass}`).toString("base64");
  const r = await fetch(creds.url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Basic ${auth}`,
    },
    body: JSON.stringify({
      jsonrpc: "1.0",
      id: "phechan",
      method,
      params,
    }),
    signal: AbortSignal.timeout(30000),
  });
  const j = await r.json();
  if (j.error) {
    throw new Error(j.error.message || JSON.stringify(j.error));
  }
  return j.result;
}

/**
 * Atomic commit+reveal: prefer bitcoind submitpackage so parent/rare sats never
 * land in a stuck commit-only state. Fall back to sequential Esplora broadcast.
 */
async function broadcastPackage(network, hexes) {
  const txs = (hexes || []).map((h) => String(h || "").trim()).filter(Boolean);
  if (txs.length < 1) throw new Error("package requires at least one tx hex");
  for (const hex of txs) {
    if (!/^[0-9a-fA-F]+$/.test(hex)) throw new Error("invalid tx hex in package");
  }
  try {
    const result = await bitcoindRpcCall(network, "submitpackage", [txs]);
    const txids = [];
    if (result && typeof result === "object") {
      const map = result.tx_results || result.package_results || {};
      for (const [k, v] of Object.entries(map)) {
        if (v && v.txid) txids.push(String(v.txid));
        else if (/^[0-9a-fA-F]{64}$/.test(k)) txids.push(k);
      }
    }
    return {
      ok: true,
      package_via: "bitcoind-submitpackage",
      txids: txids.length ? txids : undefined,
      raw: result,
    };
  } catch (pkgErr) {
    const txids = [];
    const providers = [];
    for (const hex of txs) {
      const { txid, provider } = await broadcastEsplora(network, hex);
      txids.push(txid);
      providers.push(provider);
    }
    return {
      ok: true,
      package_via: "esplora-sequential",
      package_note: `submitpackage unavailable (${pkgErr.message || pkgErr}); broadcast sequentially`,
      txids,
      providers,
    };
  }
}

function esploraBases(network) {
  const n = String(network || "").toLowerCase();
  // Same order as sort-utxo compose: emzy → memepool → mempool.space → blockstream
  if (n === "signet") {
    return [
      "https://mempool.emzy.de/signet/api",
      "https://memepool.space/signet/api",
      "https://mempool.space/signet/api",
      "https://blockstream.info/signet/api",
    ];
  }
  if (n === "testnet") {
    return [
      "https://mempool.emzy.de/testnet/api",
      "https://memepool.space/testnet/api",
      "https://mempool.space/testnet/api",
      "https://blockstream.info/testnet/api",
    ];
  }
  if (n === "mainnet" || n === "bitcoin") {
    return [
      "https://mempool.emzy.de/api",
      "https://memepool.space/api",
      "https://mempool.space/api",
      "https://blockstream.info/api",
    ];
  }
  if (n === "regtest") {
    // Local esplora-shim — POST /tx + GET /tx/:id/status (regtest-stack)
    const local = (process.env.PHECHAN_ESPLORA_URL || "http://127.0.0.1:18443").replace(
      /\/+$/,
      ""
    );
    return [local];
  }
  return [];
}

/** Broadcast raw tx hex via public Esplora (sort-utxo path). Allows <1 sat/vB. */
async function broadcastEsplora(network, txHex) {
  const hex = String(txHex || "").trim();
  if (!/^[0-9a-fA-F]+$/.test(hex)) throw new Error("invalid tx hex");
  const bases = esploraBases(network);
  if (!bases.length) throw new Error("no Esplora providers for this network");
  const errors = [];
  for (const base of bases) {
    try {
      const r = await fetch(`${base}/tx`, {
        method: "POST",
        headers: { "Content-Type": "text/plain", Accept: "text/plain" },
        body: hex,
        signal: AbortSignal.timeout(20000),
      });
      const text = (await r.text()).trim();
      if (r.ok) {
        if (/^[0-9a-fA-F]{64}$/.test(text)) return { txid: text, provider: base };
        if (!text) throw new Error("accepted but empty body");
        return { txid: text, provider: base };
      }
      errors.push(`${base} → ${r.status} ${text.slice(0, 120)}`);
    } catch (e) {
      errors.push(`${base} → ${e.message || e}`);
    }
  }
  throw new Error(`all Esplora providers failed: ${errors.join(" | ")}`);
}

/** Write upload bytes to a temp file for --body-file (avoids Windows cmdline limits). */
function writeUploadTemp(plan) {
  const buf = Buffer.from(String(plan.contentBase64), "base64");
  if (!buf.length) throw new Error("empty upload");
  let contentType = String(plan.contentType || "application/octet-stream");
  const name = String(plan.fileName || "").toLowerCase();
  if (
    (!plan.contentType || contentType === "application/octet-stream") &&
    /\.(js|mjs|cjs)$/.test(name)
  ) {
    contentType = "text/javascript";
  } else if (
    (!plan.contentType || contentType === "application/octet-stream") &&
    /\.html?$/.test(name)
  ) {
    contentType = "text/html;charset=utf-8";
  } else if (
    (!plan.contentType || contentType === "application/octet-stream") &&
    /\.txt$/.test(name)
  ) {
    contentType = "text/plain;charset=utf-8";
  }
  const safe = String(plan.fileName || "upload")
    .replace(/[^\w.\-]+/g, "_")
    .slice(0, 64);
  const tmpPath = path.join(
    fs.mkdtempSync(path.join(os.tmpdir(), "phechan-upload-")),
    safe || "body.bin"
  );
  fs.writeFileSync(tmpPath, buf);
  return { tmpPath, contentType, bytes: buf.length };
}

/** Write text to a temp file (Windows cmdline ~8KB; PSBT/base64 exceeds it). */
function writeTempUtf8(prefix, text) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  const tmpPath = path.join(dir, "data.txt");
  fs.writeFileSync(tmpPath, String(text), "utf8");
  return tmpPath;
}

/** Prefer --base64-file on Windows or when payload is large. */
function pushPsbtBase64Arg(args, b64) {
  const s = String(b64 || "");
  if (process.platform === "win32" || s.length > 2000) {
    args.push("--base64-file", writeTempUtf8("phechan-psbt-", s));
  } else {
    args.push("--base64", s);
  }
}

function pushExpectBodyArg(args, expectBody) {
  const s = String(expectBody || "");
  if (!s) {
    args.push("--skip-ordinals-check");
    return;
  }
  if (process.platform === "win32" || s.length > 1500) {
    args.push("--expect-body-file", writeTempUtf8("phechan-expect-", s));
  } else {
    args.push("--expect-body", s);
  }
}

/** Map UI inscription plan → phechan CLI args (create | delegate | reinscribe). */
/** Reveal / prepare sizing rate. Prefer revealFeeRate; fall back to legacy feeRate. */
function revealFeeRateFromPlan(plan) {
  const r = plan.revealFeeRate != null ? plan.revealFeeRate : plan.feeRate;
  return r != null && Number(r) > 0 ? String(r) : "1";
}

/** Commit funding PSBT rate. Prefer commitFeeRate; fall back to legacy feeRate. */
function commitFeeRateFromPlan(plan) {
  const r = plan.commitFeeRate != null ? plan.commitFeeRate : plan.feeRate;
  return r != null && Number(r) > 0 ? String(r) : "1";
}

function planToCliArgs(plan, { dryRun = true, unsignedPsbt = false } = {}) {
  const network = String(plan.network || "regtest");
  const mode = String(plan.mode || "text");
  const feeRate = revealFeeRateFromPlan(plan);
  const commitFeeRate = commitFeeRateFromPlan(plan);
  const postage = plan.postage != null ? String(plan.postage) : "546";

  let args;
  if (mode === "delegate") {
    const id = String(plan.delegateId || "");
    if (!id) throw new Error("delegateId required for delegate mode");
    args = ["inscription", "delegate", "--delegate", id, "--network", network];
    // No body — delegate points at existing content (ord may 404 until target exists)
  } else if (plan.satTarget && /^[0-9a-fA-F]{64}:\d+$/.test(String(plan.satTarget).trim())) {
    args = [
      "inscription",
      "reinscribe",
      "--satpoint",
      String(plan.satTarget).trim(),
      "--network",
      network,
    ];
    if (mode === "upload" && plan.contentBase64) {
      const { tmpPath, contentType } = writeUploadTemp(plan);
      args.push("--body-file", tmpPath, "--content-type", contentType);
    } else {
      args.push("--body", String(plan.body || " "));
      if (plan.contentType) args.push("--content-type", String(plan.contentType));
    }
  } else {
    let bodyText = String(plan.body || "");
    if (mode === "upload" && plan.contentBase64) {
      // Temp file + --body-file: Windows cmdline ~32KB; --body-hex cannot carry p5 (~287KB).
      const { tmpPath, contentType } = writeUploadTemp(plan);
      args = [
        "inscription",
        "create",
        "--network",
        network,
        "--body-file",
        tmpPath,
        "--content-type",
        contentType,
      ];
    }
    if (!args) {
      args = ["inscription", "create", "--body", bodyText || " ", "--network", network];
      if (plan.contentType) args.push("--content-type", String(plan.contentType));
    }
  }

  if (plan.parentId) args.push("--parent", String(plan.parentId));
  if (plan.sameSatParent) {
    args.push("--same-sat-parent");
    // Parent sits in commit — do not pass parent-outpoint (that triggers FI/FO).
  } else {
    if (plan.parentOutpoint) args.push("--parent-outpoint", String(plan.parentOutpoint));
    if (plan.parentValue != null) args.push("--parent-value", String(plan.parentValue));
    if (plan.vaultAddress) args.push("--vault-address", String(plan.vaultAddress));
    if (plan.parentAddress) args.push("--parent-address", String(plan.parentAddress));
    if (plan.parentScriptHex) args.push("--parent-script-hex", String(plan.parentScriptHex));
  }
  if (plan.ordinalsPublicKey) {
    args.push("--ordinals-pubkey-hex", String(plan.ordinalsPublicKey));
  }
  if (plan.metaprotocol) args.push("--metaprotocol", String(plan.metaprotocol));
  // Title → Properties tag 17 (--title). Metadata → tag 5. Independent fields.
  if (plan.title) args.push("--title", String(plan.title));
  if (plan.metadata) args.push("--metadata", String(plan.metadata));
  if (plan.compressBr) args.push("--compress-br");
  if (plan.opReturn) args.push("--op-return", String(plan.opReturn));
  if (plan.vanityPrefix) args.push("--vanity-prefix", String(plan.vanityPrefix));
  if (plan.vanitySuffix) args.push("--vanity-suffix", String(plan.vanitySuffix));
  if (plan.destination) args.push("--destination", String(plan.destination));
  if (plan.commitTxid) {
    args.push("--commit-txid", String(plan.commitTxid));
    if (plan.commitVout != null) args.push("--commit-vout", String(plan.commitVout));
    if (plan.commitValue != null) args.push("--commit-value", String(plan.commitValue));
    // Full commit tx so wallet can validate reveal while commit is still unbroadcast (atomic Fast).
    if (plan.commitTxHex) args.push("--commit-tx-hex", String(plan.commitTxHex));
  }
  // Sat number / inscription targeting — pass as sat-number or satpoint when verified
  if (plan.satTarget) {
    const t = String(plan.satTarget).trim();
    if (/^\d+$/.test(t)) args.push("--sat-number", t);
    else if (/^[0-9a-fA-F]{64}:\d+$/.test(t)) args.push("--satpoint-hint", t);
    else if (/^[0-9a-fA-F]{64}i\d+$/.test(t)) {
      // reinscribe intent via inscription id — CLI reinscribe needs satpoint; UI verify fills that
      args.push("--reinscribe-id", t);
    }
  }
  args.push("--fee-rate", feeRate, "--postage", postage);
  args.push("--commit-fee-rate", commitFeeRate);
  // Align commit fee estimate with inscribe.dev (carrier + payment input sizes).
  const hasCarrier = Boolean(
    plan.satTarget ||
      plan.sameSatParent ||
      (plan.carrierTxid && plan.carrierValue != null) ||
      (typeof plan.satTarget === "string" && /i\d+$/.test(String(plan.satTarget)))
  );
  if (hasCarrier) args.push("--commit-estimate-carrier");
  // Payment address only (native or nested) — do not fall back to ordinals destination.
  const payAddr = plan.paymentAddress || plan.changeAddress;
  if (payAddr) args.push("--payment-address", String(payAddr));

  if (unsignedPsbt) args.push("--unsigned-psbt");
  else if (dryRun) args.push("--dry-run");
  if (unsignedPsbt && dryRun) args.push("--dry-run");

  return args;
}

function planToRevealArgs(plan) {
  const args = planToCliArgs(plan, { dryRun: false, unsignedPsbt: false });
  const cleaned = args.filter((a) => a !== "--dry-run" && a !== "--unsigned-psbt");
  // Wallet self-custody (ordinals pubkey): always unsigned — wallet signs reveal.
  // Different-sat FI/FO: half-signed / unsigned PSBT; wallet signs parent (+ commit if custody).
  // Keystore automation (no pubkey, non-mainnet): CLI can broadcast directly.
  if (plan.ordinalsPublicKey) {
    cleaned.push("--unsigned-psbt");
  } else if (plan.sameSatParent && plan.parentId) {
    cleaned.push("--broadcast");
  } else if (plan.parentId && plan.parentOutpoint) {
    cleaned.push("--unsigned-psbt");
  } else {
    cleaned.push("--broadcast");
  }
  appendConfirmArg(cleaned, plan, plan.network);
  return cleaned;
}

function planToFundCommitArgs(plan) {
  const network = String(plan.network || "signet");
  const feeRate = commitFeeRateFromPlan(plan);
  const args = [
    "inscription",
    "fund-commit",
    "--network",
    network,
    "--commit-address",
    String(plan.commitAddress || ""),
    "--commit-sats",
    String(plan.commitSats || ""),
    "--change-address",
    String(
      plan.changeAddress ||
        plan.paymentAddress ||
        plan.carrierAddress ||
        plan.destination ||
        ""
    ),
    "--fee-rate",
    feeRate,
  ];
  // Payment funding / top-up (optional when carrier alone covers commit)
  if (plan.fundingTxid) {
    args.push(
      "--funding-txid",
      String(plan.fundingTxid),
      "--funding-vout",
      String(plan.fundingVout ?? 0),
      "--funding-value",
      String(plan.fundingValue || "")
    );
    if (plan.fundingScriptHex) args.push("--funding-script-hex", String(plan.fundingScriptHex));
    else if (plan.fundingAddress || plan.paymentAddress) {
      args.push("--funding-address", String(plan.fundingAddress || plan.paymentAddress));
    }
    if (plan.paymentPublicKey) {
      args.push("--funding-pubkey-hex", String(plan.paymentPublicKey));
    }
  }
  // Same-sat parent carrier (vin0) — parent UTXO moves into commit
  if (plan.carrierTxid) {
    args.push("--carrier-txid", String(plan.carrierTxid));
    args.push("--carrier-vout", String(plan.carrierVout ?? 0));
    args.push("--carrier-value", String(plan.carrierValue || ""));
    if (plan.carrierScriptHex) args.push("--carrier-script-hex", String(plan.carrierScriptHex));
    else if (plan.carrierAddress) args.push("--carrier-address", String(plan.carrierAddress));
  }
  if (plan.ordinalsPublicKey) {
    args.push("--ordinals-pubkey-hex", String(plan.ordinalsPublicKey));
  }
  // Commit TXID vanity (not reveal)
  if (plan.commitVanityPrefix) args.push("--vanity-prefix", String(plan.commitVanityPrefix));
  if (plan.commitVanitySuffix) args.push("--vanity-suffix", String(plan.commitVanitySuffix));
  return args;
}

function cliResult(res, result) {
  const parsed = parseCliLines(result.stdout);
  const stderr = (result.stderr || "").trim();
  const stderrError = stderr
    .split(/\r?\n/)
    .map((l) => l.trim())
    .find((l) => /^error:\s*/i.test(l));
  // Dry-run may print validation diagnostics; don't surface those as API `error`
  // when the CLI succeeded and returned a commit address.
  const fields = { ...parsed.fields };
  if (result.code === 0 && fields.commit_address) {
    delete fields.error;
  }
  const error =
    result.code === 0
      ? undefined
      : (stderrError || stderr || fields.error || `CLI exited ${result.code}`).replace(
          /^error:\s*/i,
          ""
        );
  json(res, result.code === 0 ? 200 : 400, {
    ok: result.code === 0,
    ...fields,
    raw: parsed.raw,
    stderr: stderr || undefined,
    ...(error ? { error } : {}),
  });
}

const INSCRIPTION_ID_RE = /^[0-9a-fA-F]{64}i\d+$/;
const SAT_NUMBER_RE = /^\d{1,20}$/;
const SATPOINT_RE = /^[0-9a-fA-F]{64}:\d+$/;

async function fetchJson(url, { headers = {}, timeoutMs = 15000, label = "http" } = {}) {
  const r = await fetch(url, {
    headers: { Accept: "application/json", ...headers },
    signal: AbortSignal.timeout(timeoutMs),
  });
  if (!r.ok) {
    const t = await r.text().catch(() => "");
    throw new Error(`${label} ${r.status}: ${t.slice(0, 180) || r.statusText}`);
  }
  return r.json();
}

/** Local / custom ord HTTP (signet :8080, regtest :8081, or PHECHAN_ORD_URL). */
function ordBaseForNetwork(network) {
  if (process.env.PHECHAN_ORD_URL) {
    return String(process.env.PHECHAN_ORD_URL).replace(/\/+$/, "");
  }
  const n = String(network || "").toLowerCase();
  if (n === "regtest") return "http://127.0.0.1:8081";
  // signet / testnet / unset — historical default
  return "http://127.0.0.1:8080";
}

async function localOrdGet(path, network) {
  const base = ordBaseForNetwork(network);
  return fetchJson(`${base}${path.startsWith("/") ? path : `/${path}`}`, {
    label: "local-ord",
  });
}

async function esploraOutInfo(network, txid, vout) {
  const bases = esploraBases(network);
  const errors = [];
  for (const base of bases) {
    try {
      const tx = await fetchJson(`${base}/tx/${txid}`, {
        label: "esplora",
        timeoutMs: 12000,
      });
      const out = Array.isArray(tx.vout) ? tx.vout[Number(vout)] : null;
      if (!out) throw new Error(`vout ${vout} missing`);
      return {
        address: String(out.scriptpubkey_address || ""),
        value:
          typeof out.value === "number"
            ? out.value
            : Number(out.value) || undefined,
      };
    } catch (e) {
      errors.push(String(e.message || e));
    }
  }
  throw new Error(`esplora address lookup failed: ${errors.join(" | ")}`);
}

function normalizeOrdInfo(raw, via) {
  if (!raw || typeof raw !== "object") return { via };
  const owner = raw.owner && typeof raw.owner === "object" ? raw.owner : null;
  const address = String(
    raw.address || raw.owner_address || owner?.address || ""
  );
  const output = String(
    raw.output || raw.owner_output || owner?.output || ""
  );
  let satpoint = String(raw.satpoint || "");
  if (!satpoint && output) satpoint = `${output}:0`;
  const inscriptions = Array.isArray(raw.inscriptions)
    ? raw.inscriptions
    : Array.isArray(raw.inscription_ids)
      ? raw.inscription_ids
      : raw.id
        ? [raw.id]
        : raw.inscription_id
          ? [raw.inscription_id]
          : [];
  return {
    ...raw,
    address,
    output: output || (satpoint ? satpoint.split(":").slice(0, 2).join(":") : ""),
    satpoint,
    value:
      raw.value ??
      owner?.value ??
      (typeof raw.postage === "number" ? raw.postage : undefined),
    content_type: raw.content_type || raw.contentType || undefined,
    inscriptions,
    via,
  };
}

async function ordinalsComInscription(id) {
  const raw = await fetchJson(`https://ordinals.com/r/inscription/${id}`, {
    label: "ordinals.com",
  });
  return normalizeOrdInfo(raw, "ordinals.com");
}

async function ordinalsComUtxo(outpoint) {
  const raw = await fetchJson(`https://ordinals.com/r/utxo/${outpoint}`, {
    label: "ordinals.com",
  });
  return normalizeOrdInfo(
    {
      ...raw,
      output: outpoint,
      satpoint: `${outpoint}:0`,
      inscriptions: raw.inscriptions || [],
    },
    "ordinals.com"
  );
}

async function ordinalsComSat(sat) {
  const page = await fetchJson(`https://ordinals.com/r/sat/${sat}`, {
    label: "ordinals.com",
  });
  const ids = Array.isArray(page.ids) ? page.ids : [];
  if (!ids.length) {
    throw new Error(
      "ordinals.com: sat has no inscriptions — paste satpoint (txid:vout) instead"
    );
  }
  const insc = await ordinalsComInscription(ids[0]);
  return normalizeOrdInfo(
    { ...insc, inscriptions: ids, number: Number(sat) },
    "ordinals.com"
  );
}

function ordiscanKey() {
  return (
    process.env.PHECHAN_ORDISCAN_API_KEY ||
    process.env.ORDISCAN_API_KEY ||
    ""
  ).trim();
}

async function ordiscanGet(path) {
  const key = ordiscanKey();
  if (!key) throw new Error("ordiscan: set PHECHAN_ORDISCAN_API_KEY (api.ordiscan.com requires a key)");
  const raw = await fetchJson(`https://api.ordiscan.com/v1${path}`, {
    label: "ordiscan",
    headers: { Authorization: `Bearer ${key}` },
  });
  return raw?.data && typeof raw.data === "object" ? raw.data : raw;
}

async function ordiscanInscription(id) {
  const d = await ordiscanGet(`/inscription/${id}`);
  return normalizeOrdInfo(
    {
      id,
      address: d.owner_address || d.address,
      output: d.owner_output || d.output,
      content_type: d.content_type || d.contentType,
      sat: d.sat,
      value: d.value,
    },
    "ordiscan.com"
  );
}

async function ordiscanSat(sat) {
  const d = await ordiscanGet(`/sat/${sat}`);
  const ids = Array.isArray(d.inscription_ids) ? d.inscription_ids : [];
  if (ids.length) {
    try {
      const insc = await ordiscanInscription(ids[0]);
      return normalizeOrdInfo(
        { ...insc, inscriptions: ids, number: Number(sat) },
        "ordiscan.com"
      );
    } catch {
      /* fall through */
    }
  }
  throw new Error(
    "ordiscan: sat location needs an inscription or satpoint — paste txid:vout"
  );
}

async function ordiscanUtxo(outpoint) {
  // Prefer rare-sats / sat-ranges only expose ranges; use inscription list via utxo if any.
  // Fall back: no dedicated "utxo info" — use sat-ranges for presence then Esplora for address.
  try {
    const ranges = await ordiscanGet(`/utxo/${outpoint}/sat-ranges`);
    const inscriptions = Array.isArray(ranges?.inscriptions)
      ? ranges.inscriptions
      : [];
    return normalizeOrdInfo(
      {
        output: outpoint,
        satpoint: `${outpoint}:0`,
        inscriptions,
        value: ranges?.value,
      },
      "ordiscan.com"
    );
  } catch {
    return normalizeOrdInfo(
      { output: outpoint, satpoint: `${outpoint}:0`, inscriptions: [] },
      "ordiscan.com"
    );
  }
}

/**
 * Resolve inscription / sat / utxo for Verify.
 * Mainnet: ordinals.com (free /r/) → Ordiscan (optional API key).
 * Other nets: PHECHAN_ORD_URL local ord.
 */
async function lookupOrd(kind, key, network) {
  const n = String(network || "signet").toLowerCase();
  const errors = [];

  if (isMainnet(n)) {
    if (kind === "inscription") {
      try {
        return await ordinalsComInscription(key);
      } catch (e) {
        errors.push(String(e.message || e));
      }
      try {
        return await ordiscanInscription(key);
      } catch (e) {
        errors.push(String(e.message || e));
      }
    } else if (kind === "sat") {
      try {
        return await ordinalsComSat(key);
      } catch (e) {
        errors.push(String(e.message || e));
      }
      try {
        return await ordiscanSat(key);
      } catch (e) {
        errors.push(String(e.message || e));
      }
    } else if (kind === "utxo") {
      try {
        const info = await ordinalsComUtxo(key);
        if (!info.address) {
          const [txid, vout] = key.split(":");
          const esp = await esploraOutInfo(n, txid, vout);
          info.address = esp.address;
          if (info.value == null) info.value = esp.value;
        }
        return info;
      } catch (e) {
        errors.push(String(e.message || e));
      }
      try {
        const info = await ordiscanUtxo(key);
        if (!info.address) {
          const [txid, vout] = key.split(":");
          const esp = await esploraOutInfo(n, txid, vout);
          info.address = esp.address;
          if (info.value == null) info.value = esp.value;
        }
        return info;
      } catch (e) {
        errors.push(String(e.message || e));
      }
    }
    // Optional override: still allow local/custom ord on mainnet
    if (process.env.PHECHAN_ORD_URL) {
      try {
        if (kind === "inscription") {
          return normalizeOrdInfo(
            await localOrdGet(`/inscription/${key}`, n),
            "PHECHAN_ORD_URL"
          );
        }
        if (kind === "sat") {
          return normalizeOrdInfo(await localOrdGet(`/sat/${key}`, n), "PHECHAN_ORD_URL");
        }
        if (kind === "utxo") {
          return normalizeOrdInfo(
            await localOrdGet(`/output/${key}`, n),
            "PHECHAN_ORD_URL"
          );
        }
      } catch (e) {
        errors.push(String(e.message || e));
      }
    }
    throw new Error(
      errors.join(" | ") ||
        "mainnet ord lookup failed (ordinals.com / ordiscan)"
    );
  }

  // signet / testnet / regtest — local ord (port picked from network)
  if (kind === "inscription") {
    return normalizeOrdInfo(await localOrdGet(`/inscription/${key}`, n), "local-ord");
  }
  if (kind === "sat") {
    return normalizeOrdInfo(await localOrdGet(`/sat/${key}`, n), "local-ord");
  }
  if (kind === "utxo") {
    return normalizeOrdInfo(await localOrdGet(`/output/${key}`, n), "local-ord");
  }
  throw new Error(`unknown ord kind ${kind}`);
}

function readRawBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => resolve(Buffer.concat(chunks).toString("utf8")));
    req.on("error", reject);
  });
}

function payloadHasDeniedKey(value, depth = 0) {
  if (depth > 8 || value == null) return false;
  if (Array.isArray(value)) {
    return value.some((v) => payloadHasDeniedKey(v, depth + 1));
  }
  if (typeof value === "object") {
    for (const [k, v] of Object.entries(value)) {
      const key = String(k)
        .toLowerCase()
        .replace(/[^a-z0-9_]/g, "");
      // Key names only — never scan PSBT/base64 values (false "wif" hits).
      if (
        key === "wif" ||
        key === "mnemonic" ||
        key === "seedphrase" ||
        key === "seed_phrase" ||
        key === "privatekey" ||
        key === "private_key" ||
        key.includes("privatekey")
      ) {
        return true;
      }
      if (payloadHasDeniedKey(v, depth + 1)) return true;
    }
  }
  return false;
}

async function handleApi(req, res) {
  const url = new URL(req.url || "/", `http://${HOST}`);

  // --- Studio Libraries (regtest) preview + block sim ---
  if (url.pathname === "/preview" && req.method === "GET") {
    res.writeHead(200, {
      "Content-Type": "text/html; charset=utf-8",
      "Access-Control-Allow-Origin": "*",
      "Cache-Control": "no-store",
    });
    res.end(studioPreviewHtml);
    return;
  }

  if (url.pathname === "/api/preview" && req.method === "POST") {
    studioPreviewHtml = await readRawBody(req);
    json(res, 200, { ok: true });
    return;
  }

  if (url.pathname === "/api/block-state" && req.method === "GET") {
    const info = simulateStudioBlock(studioSimBlock.height);
    json(res, 200, {
      height: studioSimBlock.height,
      enabled: studioSimBlock.enabled,
      block: info,
    });
    return;
  }

  if (url.pathname === "/api/set-block" && req.method === "POST") {
    let body;
    try {
      body = JSON.parse(await readRawBody(req) || "{}");
    } catch {
      json(res, 400, { error: "invalid json" });
      return;
    }
    if (body.height !== undefined) studioSimBlock.height = parseInt(body.height, 10);
    if (body.enabled !== undefined) studioSimBlock.enabled = Boolean(body.enabled);
    const info = simulateStudioBlock(studioSimBlock.height);
    json(res, 200, { ok: true, block: info });
    return;
  }

  if (!url.pathname.startsWith("/api/")) {
    json(res, 404, { error: "not found" });
    return;
  }

  // Hard deny key-like payloads
  if (req.method === "POST") {
    let body;
    try {
      body = await readBody(req);
    } catch {
      json(res, 400, { error: "invalid json" });
      return;
    }
    // Deny dangerous *field names* only. Do not scan values — signed PSBT
    // base64 often contains the substring "wif" and was blocking reveal.
    if (payloadHasDeniedKey(body)) {
      json(res, 400, { error: "private key / seed material is not accepted by local UI API" });
      return;
    }

    if (url.pathname === "/api/verify/parent") {
      const id = String(body.id || "").trim();
      const address = String(body.address || "").trim();
      const network = String(body.network || "signet").toLowerCase();
      if (!INSCRIPTION_ID_RE.test(id)) {
        json(res, 400, { ok: false, error: "Invalid inscription ID. Format: <64-hex>i<index>" });
        return;
      }
      if (!address) {
        json(res, 400, { ok: false, error: "Connect wallet first — ownership check needs your address." });
        return;
      }
      try {
        const info = await lookupOrd("inscription", id, network);
        const satpoint = String(info.satpoint || info.output || "");
        const [txid, voutStr] = satpoint.split(":");
        const vout = parseInt(voutStr, 10);
        let owner = String(info.address || "");
        let value = info.value ?? undefined;
        if ((!owner || value == null) && txid && Number.isFinite(vout)) {
          try {
            const esp = await esploraOutInfo(network, txid, vout);
            if (!owner) owner = esp.address;
            if (value == null) value = esp.value;
          } catch {
            /* keep ord fields */
          }
        }
        const owned = owner === address;
        json(res, 200, {
          ok: true,
          owned,
          address: owner,
          txid,
          vout,
          value,
          via: info.via,
          detail: owned
            ? `Verified via ${info.via} — you hold parent at ${txid}:${vout}`
            : `Parent found via ${info.via} but owned by ${owner || "unknown"} — not your connected address`,
          error: owned ? undefined : "Not owned by connected wallet",
        });
      } catch (e) {
        json(res, 400, {
          ok: false,
          error:
            String(e.message || e) +
            (isMainnet(network)
              ? " — mainnet uses ordinals.com (then Ordiscan if PHECHAN_ORDISCAN_API_KEY is set)"
              : " — set PHECHAN_ORD_URL to your network's ord"),
        });
      }
      return;
    }

    if (url.pathname === "/api/verify/delegate") {
      const id = String(body.id || "").trim();
      const network = String(body.network || "signet").toLowerCase();
      if (!INSCRIPTION_ID_RE.test(id)) {
        json(res, 400, { ok: false, error: "Invalid inscription ID. Format: <64-hex>i<index>" });
        return;
      }
      try {
        const info = await lookupOrd("inscription", id, network);
        json(res, 200, {
          ok: true,
          contentType: info.content_type || info.contentType || "unknown",
          via: info.via,
          detail: `Delegate target exists via ${info.via} (${info.content_type || info.contentType || "type unknown"})`,
        });
      } catch (e) {
        json(res, 400, {
          ok: false,
          error:
            String(e.message || e) +
            (isMainnet(network)
              ? " — mainnet uses ordinals.com (then Ordiscan if PHECHAN_ORDISCAN_API_KEY is set)"
              : " — set PHECHAN_ORD_URL to your network's ord"),
        });
      }
      return;
    }

    if (url.pathname === "/api/verify/sat-target") {
      const input = String(body.input || "").trim();
      const address = String(body.address || "").trim();
      const network = String(body.network || "signet").toLowerCase();
      if (!address) {
        json(res, 400, { ok: false, error: "Connect wallet first — ownership check needs your address." });
        return;
      }
      try {
        let info;
        let kind;
        if (INSCRIPTION_ID_RE.test(input)) {
          kind = "inscription";
          info = await lookupOrd("inscription", input, network);
        } else if (SAT_NUMBER_RE.test(input)) {
          kind = "sat";
          info = await lookupOrd("sat", input, network);
        } else if (SATPOINT_RE.test(input)) {
          kind = "satpoint";
          const [txid, vout] = input.split(":");
          info = await lookupOrd("utxo", `${txid}:${vout}`, network);
        } else {
          json(res, 400, {
            ok: false,
            error: "Enter a sat number (digits), inscription ID (<64-hex>i<n>), or satpoint (txid:vout).",
          });
          return;
        }

        const satpoint = String(info.satpoint || (kind === "satpoint" ? `${input}:0` : "") || "");
        const parts = satpoint.split(":");
        const txid = parts[0];
        const vout = parseInt(parts[1], 10);
        const offset = parts[2] != null ? parseInt(parts[2], 10) : 0;
        let owner = String(info.address || "");
        let value = info.value ?? undefined;
        if ((!owner || value == null) && txid && Number.isFinite(vout)) {
          try {
            const esp = await esploraOutInfo(network, txid, vout);
            if (!owner) owner = esp.address;
            if (value == null) value = esp.value;
          } catch {
            /* keep */
          }
        }
        const owned = !owner || owner === address;
        const inscriptionIds = Array.isArray(info.inscriptions)
          ? info.inscriptions.map((x) => (typeof x === "string" ? x : x?.id)).filter(Boolean)
          : info.inscription_id
            ? [info.inscription_id]
            : kind === "inscription"
              ? [input]
              : [];
        const reinscribe = inscriptionIds.length > 0;
        if (offset !== 0 && kind === "sat") {
          json(res, 200, {
            ok: false,
            owned,
            error: `Sat is at offset ${offset} of its UTXO, not 0. Split so the target sat is first, then retry.`,
            txid,
            vout,
            satNumber: kind === "sat" ? Number(input) : null,
            inscriptionIds,
            reinscribe,
            via: info.via,
          });
          return;
        }
        json(res, 200, {
          ok: owned,
          owned,
          txid,
          vout,
          value,
          address: owner || address,
          satNumber: kind === "sat" ? Number(input) : info.number ?? info.sat ?? null,
          inscriptionIds,
          reinscribe,
          via: info.via,
          detail: owned
            ? reinscribe
              ? `Verified via ${info.via} — you hold this UTXO; reinscription (already has ${inscriptionIds.length} inscription(s))`
              : `Verified via ${info.via} — target in your wallet at ${txid}:${vout}`
            : `Target found via ${info.via} but owned by ${owner || "unknown"}`,
          error: owned ? undefined : "Not owned by connected wallet",
        });
      } catch (e) {
        json(res, 400, {
          ok: false,
          error:
            String(e.message || e) +
            (isMainnet(network)
              ? " — mainnet uses ordinals.com (then Ordiscan if PHECHAN_ORDISCAN_API_KEY is set)"
              : " — set PHECHAN_ORD_URL to your network's ord"),
        });
      }
      return;
    }

    if (url.pathname === "/api/inscription/preview") {
      try {
        const args = planToCliArgs(body, { dryRun: true, unsignedPsbt: false });
        const result = await runPhechan(args);
        cliResult(res, result);
      } catch (e) {
        json(res, 400, { error: String(e.message || e) });
      }
      return;
    }

    if (url.pathname === "/api/inscription/prepare") {
      try {
        const network = String(body.network || "signet");
        requireWalletCustodyPubkey(body, network);
        const args = planToCliArgs(body, { dryRun: true, unsignedPsbt: false });
        const result = await runPhechan(args);
        cliResult(res, result);
      } catch (e) {
        json(res, 400, { error: String(e.message || e) });
      }
      return;
    }

    if (url.pathname === "/api/inscription/brotli-preview") {
      try {
        let raw;
        if (body.contentBase64) {
          raw = Buffer.from(String(body.contentBase64), "base64");
        } else {
          raw = Buffer.from(String(body.body || ""), "utf8");
        }
        const compressed = zlib.brotliCompressSync(raw, {
          params: {
            [zlib.constants.BROTLI_PARAM_QUALITY]: 5,
          },
        });
        const feeRate =
          Number(body.revealFeeRate) > 0
            ? Number(body.revealFeeRate)
            : Number(body.feeRate) > 0
              ? Number(body.feeRate)
              : 1;
        const savedBytes = Math.max(0, raw.length - compressed.length);
        // Body lives in witness → ~1 weight unit per byte → vbytes ≈ bytes/4
        const savedVbytes = savedBytes / 4;
        const savedFeeSats = savedVbytes * feeRate;
        json(res, 200, {
          ok: true,
          rawBytes: raw.length,
          compressedBytes: compressed.length,
          savedBytes,
          savedPct: raw.length ? Math.round((savedBytes / raw.length) * 1000) / 10 : 0,
          savedVbytes: Math.round(savedVbytes * 10) / 10,
          savedFeeSats: Math.round(savedFeeSats * 100) / 100,
          feeRate,
          note: "Estimate: witness body savings ≈ bytes/4 vB × fee rate. Envelope/tag overhead unchanged.",
        });
      } catch (e) {
        json(res, 400, { error: String(e.message || e) });
      }
      return;
    }

    if (url.pathname === "/api/inscription/fund-psbt") {
      try {
        if (!body.commitAddress || !body.commitSats) {
          json(res, 400, { error: "commitAddress and commitSats required" });
          return;
        }
        if (!body.carrierTxid && (!body.fundingTxid || body.fundingValue == null)) {
          json(res, 400, {
            error: "fundingTxid+fundingValue required (or carrierTxid for same-sat parent)",
          });
          return;
        }
        const network = String(body.network || "signet");
        requireWalletCustodyPubkey(body, network);
        const args = planToFundCommitArgs(body);
        const result = await runPhechan(args, { timeoutMs: 300_000 });
        cliResult(res, result);
      } catch (e) {
        json(res, 400, { error: String(e.message || e) });
      }
      return;
    }

    if (url.pathname === "/api/inscription/reveal") {
      try {
        if (!body.commitTxid) {
          json(res, 400, { error: "commitTxid required (fund commit via wallet first)" });
          return;
        }
        if (!body.destination) {
          json(res, 400, { error: "destination address required" });
          return;
        }
        const net = String(body.network || "signet").toLowerCase();
        assertMainnetGate(body, net);
        requireWalletCustodyPubkey(body, net);
        if (body.parentId && !body.parentOutpoint && !body.sameSatParent) {
          json(res, 400, {
            error:
              "Parent ID set but not Verified — Verify parent so we can spend it. Tag 3 alone is not a real child.",
          });
          return;
        }
        if (body.parentId && body.parentOutpoint) {
          if (body.parentValue == null) {
            json(res, 400, { error: "parentValue required (from Verify)" });
            return;
          }
          if (!body.vaultAddress) {
            json(res, 400, {
              error: "vaultAddress required — where the parent inscription returns after reveal",
            });
            return;
          }
        }
        const args = planToRevealArgs(body);
        const result = await runPhechan(args, { timeoutMs: 300_000, network: net });
        cliResult(res, result);
      } catch (e) {
        json(res, 400, { error: String(e.message || e) });
      }
      return;
    }

    if (url.pathname === "/api/inscription/export-psbt") {
      try {
        const args = planToCliArgs(body, { dryRun: true, unsignedPsbt: true });
        const result = await runPhechan(args);
        cliResult(res, result);
      } catch (e) {
        json(res, 400, { error: String(e.message || e) });
      }
      return;
    }

    if (url.pathname === "/api/inscription/create-dry-run") {
      const text = String(body.body || "Hello, world!");
      const network = String(body.network || "regtest");
      const result = await runPhechan([
        "inscription",
        "create",
        "--body",
        text,
        "--network",
        network,
        "--dry-run",
      ]);
      const parsed = parseCliLines(result.stdout);
      json(res, result.code === 0 ? 200 : 400, {
        ok: result.code === 0,
        ...parsed.fields,
        raw: parsed.raw,
        stderr: result.stderr.trim() || undefined,
      });
      return;
    }

    if (url.pathname === "/api/inscription/child-dry-run") {
      const text = String(body.body || "child");
      const parent = String(body.parent || "");
      if (!parent) {
        json(res, 400, { error: "parent required" });
        return;
      }
      const args = [
        "inscription",
        "child",
        "--body",
        text,
        "--parent",
        parent,
        "--network",
        String(body.network || "regtest"),
        "--dry-run",
        "--placement",
        String(body.placement || "fifo"),
      ];
      if (body.parentHasRunes) args.push("--parent-has-runes");
      const result = await runPhechan(args);
      const parsed = parseCliLines(result.stdout);
      json(res, result.code === 0 ? 200 : 400, {
        ok: result.code === 0,
        ...parsed.fields,
        raw: parsed.raw,
        stderr: result.stderr.trim() || undefined,
      });
      return;
    }

    if (url.pathname === "/api/inscription/inspect") {
      const text = String(body.body || "");
      if (!text) {
        json(res, 400, { error: "body required" });
        return;
      }
      const result = await runPhechan(["inscription", "inspect", "--body", text]);
      const parsed = parseCliLines(result.stdout);
      json(res, result.code === 0 ? 200 : 400, {
        ok: result.code === 0,
        ...parsed.fields,
        raw: parsed.raw,
        stderr: result.stderr.trim() || undefined,
      });
      return;
    }

    if (url.pathname === "/api/inscription/create-broadcast") {
      const text = String(body.body || "Hello, world!");
      const network = String(body.network || "regtest");
      const confirm = String(body.confirm || "");
      if (network !== "regtest") {
        json(res, 400, {
          error: "UI broadcast is regtest-only; use CLI tx broadcast gates for other nets",
        });
        return;
      }
      if (confirm !== "BROADCAST REGTEST") {
        json(res, 400, {
          error: 'type confirm exactly: BROADCAST REGTEST',
        });
        return;
      }
      const result = await runPhechan(
        [
          "inscription",
          "create",
          "--body",
          text,
          "--network",
          network,
          "--broadcast",
        ],
        { timeoutMs: 180_000 }
      );
      const parsed = parseCliLines(result.stdout);
      json(res, result.code === 0 ? 200 : 400, {
        ok: result.code === 0,
        ...parsed.fields,
        raw: parsed.raw,
        stderr: result.stderr.trim() || undefined,
      });
      return;
    }

    if (url.pathname === "/api/psbt/finalize-import") {
      try {
        const b64 = String(body.base64 || body.psbt || "");
        if (!b64) {
          json(res, 400, { error: "base64 required" });
          return;
        }
        const network = String(body.network || "regtest").toLowerCase();
        const expectBody = body.expectBody != null ? String(body.expectBody) : "";
        const doBroadcast = Boolean(body.broadcast);
        const args = [
          "psbt",
          "finalize-import",
          "--network",
          network,
        ];
        pushPsbtBase64Arg(args, b64);
        if (expectBody) {
          pushExpectBodyArg(args, expectBody);
        } else {
          args.push("--skip-ordinals-check");
        }
        if (doBroadcast) {
          args.push("--broadcast");
          appendConfirmArg(args, body, network);
        }
        const result = await runPhechan(args, { timeoutMs: 180_000, network });
        const parsed = parseCliLines(result.stdout);
        json(res, result.code === 0 ? 200 : 400, {
          ok: result.code === 0,
          ...parsed.fields,
          raw: parsed.raw,
          stderr: result.stderr.trim() || undefined,
        });
      } catch (e) {
        json(res, 400, { error: String(e.message || e) });
      }
      return;
    }

    if (url.pathname === "/api/psbt/inspect") {
      try {
        const b64 = String(body.base64 || body.psbt || "");
        if (!b64) {
          json(res, 400, { error: "base64 required" });
          return;
        }
        const network = String(body.network || "signet").toLowerCase();
        const args = [
          "psbt",
          "inspect",
          "--network",
          network,
        ];
        pushPsbtBase64Arg(args, b64);
        const result = await runPhechan(args, { timeoutMs: 60_000, network });
        const parsed = parseCliLines(result.stdout);
        const vin = [];
        const vout = [];
        for (const line of parsed.raw) {
          if (/^vin\[\d+\]:/.test(line)) vin.push(line.replace(/^vin\[\d+\]:\s*/, ""));
          else if (/^vout\[\d+\]:/.test(line)) vout.push(line.replace(/^vout\[\d+\]:\s*/, ""));
        }
        json(res, result.code === 0 ? 200 : 400, {
          ok: result.code === 0,
          ...parsed.fields,
          vin,
          vout,
          raw: parsed.raw,
          stderr: result.stderr.trim() || undefined,
        });
      } catch (e) {
        json(res, 400, { error: String(e.message || e) });
      }
      return;
    }

    if (url.pathname === "/api/psbt/finalize-funding") {
      try {
        const b64 = String(body.base64 || body.psbt || "");
        if (!b64) {
          json(res, 400, { error: "base64 required" });
          return;
        }
        const network = String(body.network || "signet").toLowerCase();
        if (body.broadcast !== false) {
          assertMainnetGate(body, network);
        }
        // Finalize only in CLI (nested redeem + extract hex). Broadcast via Node
        // Esplora like sort-utxo — ureq often times out / misses memepool.space.
        const args = [
          "psbt",
          "finalize-import",
          "--network",
          network,
          "--skip-ordinals-check",
        ];
        pushPsbtBase64Arg(args, b64);
        appendConfirmArg(args, body, network);
        if (body.paymentPublicKey) {
          args.push("--funding-pubkey-hex", String(body.paymentPublicKey));
        }
        // No --broadcast: we push hex ourselves
        const result = await runPhechan(args, { timeoutMs: 180_000, network });
        const parsed = parseCliLines(result.stdout);
        if (result.code !== 0) {
          cliResult(res, result);
          return;
        }
        const hex = String(parsed.fields.hex || "");
        const plannedTxid = String(parsed.fields.txid || "");
        if (!hex) {
          json(res, 400, {
            ok: false,
            error: "finalize produced no hex",
            ...parsed.fields,
            raw: parsed.raw,
          });
          return;
        }
        if (body.broadcast === false) {
          json(res, 200, {
            ok: true,
            ...parsed.fields,
            raw: parsed.raw,
          });
          return;
        }
        try {
          const { txid, provider } = await broadcastEsplora(network, hex);
          json(res, 200, {
            ok: true,
            ...parsed.fields,
            broadcast_txid: txid,
            broadcast_via: provider,
            txid: plannedTxid || txid,
            raw: parsed.raw,
          });
        } catch (e) {
          json(res, 400, {
            ok: false,
            error: String(e.message || e),
            txid: plannedTxid || undefined,
            hex,
            raw: parsed.raw,
          });
        }
      } catch (e) {
        json(res, 400, { error: String(e.message || e) });
      }
      return;
    }

    if (url.pathname === "/api/tx/submit-package") {
      try {
        const network = String(body.network || "signet").toLowerCase();
        assertMainnetGate(body, network);
        const hexes = Array.isArray(body.hexes)
          ? body.hexes
          : [body.commitHex, body.revealHex].filter(Boolean);
        if (hexes.length < 2) {
          json(res, 400, { error: "commitHex + revealHex (or hexes[]) required" });
          return;
        }
        const result = await broadcastPackage(network, hexes);
        json(res, 200, result);
      } catch (e) {
        json(res, 400, { ok: false, error: String(e.message || e) });
      }
      return;
    }

    if (url.pathname === "/api/inscription/unsigned-psbt") {
      const text = String(body.body || "Hello, world!");
      const network = String(body.network || "regtest");
      const result = await runPhechan([
        "inscription",
        "create",
        "--body",
        text,
        "--network",
        network,
        "--dry-run",
        "--unsigned-psbt",
      ]);
      const parsed = parseCliLines(result.stdout);
      json(res, result.code === 0 ? 200 : 400, {
        ok: result.code === 0,
        ...parsed.fields,
        raw: parsed.raw,
        stderr: result.stderr.trim() || undefined,
      });
      return;
    }

    if (url.pathname === "/api/sat/select") {
      const inputs = String(body.inputs || "");
      const outputs = String(body.outputs || "");
      const at = String(body.at || "0:0");
      const result = await runPhechan([
        "sat",
        "select",
        "--inputs",
        inputs,
        "--outputs",
        outputs,
        "--at",
        at,
      ]);
      const parsed = parseCliLines(result.stdout);
      json(res, result.code === 0 ? 200 : 400, {
        ok: result.code === 0,
        ...parsed.fields,
        raw: parsed.raw,
        stderr: result.stderr.trim() || undefined,
      });
      return;
    }

    json(res, 404, { error: "unknown POST route" });
    return;
  }

  if (req.method === "GET" && url.pathname === "/api/health") {
    json(res, 200, {
      ok: true,
      bind: HOST,
      note: "local-only; no key APIs",
      mainnetUnlocked: true,
      ordMainnet: "ordinals.com/r → ordiscan (optional PHECHAN_ORDISCAN_API_KEY) → PHECHAN_ORD_URL",
      ordOther: process.env.PHECHAN_ORD_URL || "regtest→:8081 signet→:8080",
      ordiscanKeyConfigured: Boolean(ordiscanKey()),
    });
    return;
  }

  if (req.method === "GET" && url.pathname === "/api/utxo/list") {
    const withOrd = url.searchParams.get("ord") === "1" || url.searchParams.get("ord") === "true";
    const args = ["utxo", "list"];
    if (withOrd) args.push("--ord");
    const result = await runPhechan(args);
    const parsed = parseCliLines(result.stdout);
    json(res, result.code === 0 ? 200 : 400, {
      ok: result.code === 0,
      ...parsed.fields,
      raw: result.stdout.split(/\r?\n/).filter(Boolean),
      stderr: result.stderr.trim() || undefined,
    });
    return;
  }

  if (req.method === "GET" && url.pathname === "/api/utxo/for-address") {
    const address = String(url.searchParams.get("address") || "").trim();
    const network = String(url.searchParams.get("network") || "signet").toLowerCase();
    if (!address || address.length < 10) {
      json(res, 400, { ok: false, error: "address required" });
      return;
    }
    const providers =
      network === "mainnet" || network === "bitcoin"
        ? [
            "https://mempool.emzy.de/api",
            "https://mempool.space/api",
            "https://blockstream.info/api",
          ]
        : network === "testnet"
          ? [
              "https://mempool.space/testnet/api",
              "https://blockstream.info/testnet/api",
            ]
          : network === "regtest"
            ? [
                // Local esplora-shim (regtest-stack start-esplora.ps1) — not public esplora
                (process.env.PHECHAN_ESPLORA_URL || "http://127.0.0.1:18443").replace(
                  /\/+$/,
                  ""
                ),
              ]
            : [
                "https://mempool.emzy.de/signet/api",
                "https://mempool.space/signet/api",
                "https://blockstream.info/signet/api",
              ];
    if (!providers.length) {
      json(res, 400, {
        ok: false,
        error: "regtest UTXOs: use local bitcoind (no public esplora)",
      });
      return;
    }
    let lastErr = "";
    for (const base of providers) {
      try {
        const r = await fetch(`${base}/address/${encodeURIComponent(address)}/utxo`, {
          headers: { Accept: "application/json" },
          signal: AbortSignal.timeout(15000),
        });
        if (!r.ok) {
          lastErr = `${base} → ${r.status}`;
          if (r.status >= 500) continue;
          const t = await r.text().catch(() => "");
          lastErr = `${base} → ${r.status} ${t.slice(0, 120)}`;
          continue;
        }
        const arr = await r.json();
        if (!Array.isArray(arr)) {
          lastErr = `${base} → invalid JSON`;
          continue;
        }
        let tipHeight = null;
        if (network === "regtest") {
          try {
            const tipRes = await fetch(`${base}/blocks/tip/height`, {
              signal: AbortSignal.timeout(5000),
            });
            if (tipRes.ok) tipHeight = Number((await tipRes.text()).trim());
          } catch {
            /* keep all if tip unknown */
          }
        }
        const COINBASE_MATURITY = 100;
        let skippedImmature = 0;
        const mapped = arr
          .map((u) => {
            const bh = u?.status?.block_height;
            const conf =
              tipHeight != null && bh != null && Number.isFinite(Number(bh))
                ? tipHeight - Number(bh) + 1
                : null;
            return {
              txid: String(u.txid || ""),
              vout: Number(u.vout ?? 0),
              value: Number(u.value ?? 0),
              address,
              confirmations: conf,
            };
          })
          .filter((u) => /^[0-9a-f]{64}$/i.test(u.txid) && u.value > 0);

        // Only hide immature *coinbase* outputs (100-conf rule). Payment change is
        // spendable immediately — do not treat "conf < 100" as coinbase.
        const coinbaseCache = new Map();
        async function txIsCoinbase(txid) {
          if (coinbaseCache.has(txid)) return coinbaseCache.get(txid);
          try {
            const tr = await fetch(`${base}/tx/${encodeURIComponent(txid)}`, {
              headers: { Accept: "application/json" },
              signal: AbortSignal.timeout(8000),
            });
            if (!tr.ok) {
              coinbaseCache.set(txid, false);
              return false;
            }
            const tx = await tr.json();
            const vin0 = Array.isArray(tx?.vin) ? tx.vin[0] : null;
            const isCb = Boolean(
              vin0 && (vin0.is_coinbase === true || typeof vin0.coinbase === "string")
            );
            coinbaseCache.set(txid, isCb);
            return isCb;
          } catch {
            coinbaseCache.set(txid, false);
            return false;
          }
        }

        const utxos = [];
        for (const u of mapped) {
          if (
            network === "regtest" &&
            tipHeight != null &&
            u.confirmations != null &&
            u.confirmations < COINBASE_MATURITY
          ) {
            const isCb = await txIsCoinbase(u.txid);
            if (isCb) {
              skippedImmature += 1;
              continue;
            }
          }
          utxos.push(u);
        }
        utxos.sort((a, b) => b.value - a.value);
        json(res, 200, {
          ok: true,
          utxos,
          provider: base,
          message: utxos.length
            ? `Found ${utxos.length} UTXO(s) on ${network}${
                skippedImmature
                  ? ` (${skippedImmature} immature coinbase hidden — need 100 conf)`
                  : ""
              }`
            : `No UTXOs on ${address} (${network})${
                skippedImmature ? ` — ${skippedImmature} immature coinbase only` : ""
              }`,
        });
        return;
      } catch (e) {
        lastErr = String(e.message || e);
      }
    }
    json(res, 400, { ok: false, error: `UTXO fetch failed: ${lastErr || "all providers down"}` });
    return;
  }

  json(res, 404, { error: "not found" });
}

async function main() {
  const api = http.createServer((req, res) => {
    handleApi(req, res).catch((err) => {
      json(res, 500, { error: String(err?.message || err) });
    });
  });

  await new Promise((resolve) => api.listen(API_PORT, HOST, resolve));
  console.log(`Phechan API  http://${HOST}:${API_PORT}  (localhost only)`);
  console.log(`Studio Libraries proxies ord @ ${ORD_URL} via Vite /content + /r`);

  const vite = await createViteServer({
    root: ROOT,
    server: {
      host: HOST,
      port: UI_PORT,
      strictPort: true,
      proxy: {
        "/api": `http://${HOST}:${API_PORT}`,
        "/preview": `http://${HOST}:${API_PORT}`,
        "/content": {
          target: ORD_URL,
          changeOrigin: true,
          configure: (proxy) => {
            proxy.on("proxyReq", (proxyReq) => {
              proxyReq.setHeader("Accept-Encoding", "br, gzip, deflate, identity");
              proxyReq.setHeader("Accept", "*/*");
            });
          },
        },
        "/r": {
          target: ORD_URL,
          changeOrigin: true,
          configure: (proxy) => {
            proxy.on("proxyReq", (proxyReq) => {
              proxyReq.setHeader("Accept-Encoding", "br, gzip, deflate, identity");
              proxyReq.setHeader("Accept", "*/*");
            });
          },
        },
      },
    },
  });
  await vite.listen();
  console.log(`Phechan UI   http://${HOST}:${UI_PORT}`);
  console.log(`Studio Libraries (regtest)  http://${HOST}:${UI_PORT}/studio-regtest/`);
  console.log("No private-key endpoints. Ctrl+C to stop.");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
