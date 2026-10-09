/** Xverse / Bitcoin provider helpers. No private keys leave the wallet. */

export type WalletStatus = {
  available: boolean;
  name: string;
  detail?: string;
};

export type WalletSession = {
  ok: boolean;
  message: string;
  /** Ordinals / taproot receive (reveal destination) */
  address?: string;
  /** Payment address for funding commit (often nested segwit) */
  paymentAddress?: string;
  /** Payment pubkey hex (33-byte compressed or 32-byte x-only) — required for nested P2SH */
  paymentPublicKey?: string;
  /** Ordinals / taproot x-only pubkey — required to sign parent in parent-child reveal */
  ordinalsPublicKey?: string;
  /** Detected from address prefix */
  paymentAddressType?: "p2wpkh" | "p2sh-p2wpkh" | "p2tr" | "unknown";
  addresses?: string[];
  network?: string;
  provider?: string;
};

export type WalletUtxo = {
  txid: string;
  vout: number;
  value: number;
  scriptPubKey?: string;
  address?: string;
  /** Set on regtest when esplora tip height is known */
  confirmations?: number | null;
};

type Provider = {
  request: (method: string, params?: unknown) => Promise<unknown>;
};

function getProvider(): Provider | null {
  const w = window as Window & {
    XverseProviders?: { Bitcoin?: Provider };
    BitcoinProvider?: Provider;
  };
  return w.XverseProviders?.Bitcoin || w.BitcoinProvider || null;
}

export function detectWallet(): WalletStatus {
  const p = getProvider();
  if (!p) {
    return {
      available: false,
      name: "none",
      detail: "Install Xverse (or compatible) and reload.",
    };
  }
  return { available: true, name: "Xverse / BitcoinProvider" };
}

function asRecord(v: unknown): Record<string, unknown> | null {
  return v && typeof v === "object" ? (v as Record<string, unknown>) : null;
}

function pickAddresses(payload: unknown): {
  ordinals?: string;
  payment?: string;
  paymentPublicKey?: string;
  ordinalsPublicKey?: string;
  addresses: string[];
} {
  const addresses: string[] = [];
  let ordinals: string | undefined;
  let payment: string | undefined;
  let paymentPublicKey: string | undefined;
  let ordinalsPublicKey: string | undefined;

  const push = (s: unknown) => {
    if (typeof s === "string" && s.length > 10) addresses.push(s);
  };

  const takePubkey = (o: Record<string, unknown>) => {
    const pk = o.publicKey ?? o.pubkey;
    if (typeof pk === "string" && /^[0-9a-fA-F]{64,66}$/.test(pk)) {
      return pk;
    }
    return undefined;
  };

  const walk = (v: unknown) => {
    if (!v) return;
    if (typeof v === "string") {
      push(v);
      return;
    }
    if (Array.isArray(v)) {
      for (const item of v) {
        if (typeof item === "string") push(item);
        else {
          const o = asRecord(item);
          if (o) {
            push(o.address);
            push(o.paymentAddress);
            push(o.ordinalsAddress);
            push(o.cardinal);
            if (typeof o.ordinalsAddress === "string") ordinals = o.ordinalsAddress;
            if (typeof o.paymentAddress === "string") payment = o.paymentAddress;
            const purpose = String(o.purpose || o.addressType || "").toLowerCase();
            if (typeof o.address === "string") {
              if (purpose.includes("ordinal") || /^(bc1p|tb1p|bcrt1p)/i.test(o.address)) {
                ordinals = ordinals || o.address;
                const pk = takePubkey(o);
                if (pk) ordinalsPublicKey = pk;
              }
              if (
                purpose.includes("payment") ||
                purpose.includes("cardinal") ||
                purpose === "p2sh" ||
                purpose === "p2wpkh"
              ) {
                payment = payment || o.address;
                const pk = takePubkey(o);
                if (pk) paymentPublicKey = pk;
              }
            }
            const at = String(o.addressType || "").toLowerCase();
            if (at === "p2tr" || at.includes("ordinal")) {
              ordinalsPublicKey = ordinalsPublicKey || takePubkey(o);
            }
            if (at === "p2sh" || at === "p2wpkh" || at.includes("payment")) {
              paymentPublicKey = paymentPublicKey || takePubkey(o);
            }
          }
        }
      }
      return;
    }
    const o = asRecord(v);
    if (!o) return;
    if (o.result !== undefined) walk(o.result);
    if (o.addresses !== undefined) walk(o.addresses);
    push(o.address);
    push(o.paymentAddress);
    push(o.ordinalsAddress);
    if (typeof o.ordinalsAddress === "string") ordinals = o.ordinalsAddress;
    if (typeof o.paymentAddress === "string") payment = o.paymentAddress;
    if (typeof o.paymentPublicKey === "string") paymentPublicKey = o.paymentPublicKey;
  };

  walk(payload);
  const uniq = [...new Set(addresses)];
  if (!ordinals) ordinals = uniq.find((a) => /^(bc1p|tb1p|bcrt1p)/i.test(a));
  if (!payment) {
    payment =
      uniq.find((a) => /^(bc1q|tb1q|bcrt1q|2|3|m|n|1)/i.test(a) && a !== ordinals) ||
      uniq.find((a) => a !== ordinals);
  }
  return {
    ordinals: ordinals || uniq[0],
    payment: payment || ordinals || uniq[0],
    paymentPublicKey,
    ordinalsPublicKey,
    addresses: uniq,
  };
}

export function paymentAddressType(
  address?: string
): "p2wpkh" | "p2sh-p2wpkh" | "p2tr" | "unknown" {
  if (!address) return "unknown";
  if (/^(bc1p|tb1p|bcrt1p)/i.test(address)) return "p2tr";
  if (/^(bc1q|tb1q|bcrt1q)/i.test(address)) return "p2wpkh";
  if (/^[23]/i.test(address)) return "p2sh-p2wpkh";
  return "unknown";
}

function normalizeNetwork(raw: unknown, addressHint?: string): string | undefined {
  if (typeof raw === "string") {
    const s = raw.toLowerCase();
    if (s.includes("signet")) return "signet";
    if (s.includes("main")) return "mainnet";
    if (s.includes("reg")) return "regtest";
    // Xverse reports Signet as "Testnet" / "Testnet4" — same address prefixes as sort-utxo
    if (s.includes("testnet4") || s === "testnet" || s.includes("test")) {
      if (addressHint && /^(tb1|2|m|n)/i.test(addressHint)) return "signet";
      return "signet"; // Xverse non-mainnet default for Phechan local UI
    }
    return s;
  }
  const o = asRecord(raw);
  if (!o) return undefined;
  if (o.result !== undefined) return normalizeNetwork(o.result, addressHint);
  if (o.network !== undefined) return normalizeNetwork(o.network, addressHint);
  if (typeof o.name === "string") return normalizeNetwork(o.name, addressHint);
  if (typeof o.bitcoin === "object") {
    const b = asRecord(o.bitcoin);
    if (b?.name) return normalizeNetwork(b.name, addressHint);
  }
  return undefined;
}

async function tryRequest(p: Provider, method: string, params?: unknown): Promise<unknown> {
  try {
    return await p.request(method, params);
  } catch {
    return null;
  }
}

/** Connect and auto-fetch payment/ordinals address + network. */
export async function connectWallet(): Promise<WalletSession> {
  const p = getProvider();
  if (!p) return { ok: false, message: "No wallet provider detected" };

  // Match runes-etch / sort-utxo: request Ordinals + Payment so publicKey is returned.
  let connectRes =
    (await tryRequest(p, "wallet_connect", {
      addresses: ["ordinals", "payment"],
      message: "Connect to Phechan local inscription UI",
    })) ??
    (await tryRequest(p, "wallet_connect", {
      purposes: ["ordinals", "payment"],
    })) ??
    (await tryRequest(p, "wallet_connect", null)) ??
    (await tryRequest(p, "requestAccounts", undefined)) ??
    (await tryRequest(p, "getAccounts", undefined));

  if (!connectRes) {
    return { ok: false, message: "Connect failed — unlock Xverse and try again" };
  }

  let picked = pickAddresses(connectRes);

  // Always enrich if missing ordinals OR payment pubkey (needed for nested P2SH).
  if (!picked.ordinals || !picked.paymentPublicKey || !picked.ordinalsPublicKey) {
    const more =
      (await tryRequest(p, "getAddresses", {
        purposes: ["ordinals", "payment"],
        message: "Phechan needs payment + ordinals public keys",
      })) ??
      (await tryRequest(p, "wallet_getAccount", null)) ??
      (await tryRequest(p, "getAccounts", undefined));
    const again = pickAddresses(more);
    picked = {
      ordinals: again.ordinals || picked.ordinals,
      payment: again.payment || picked.payment,
      paymentPublicKey: again.paymentPublicKey || picked.paymentPublicKey,
      ordinalsPublicKey: again.ordinalsPublicKey || picked.ordinalsPublicKey,
      addresses: [...new Set([...picked.addresses, ...again.addresses])],
    };
  }

  const netRaw =
    (await tryRequest(p, "wallet_getNetwork", null)) ??
    (await tryRequest(p, "getNetwork", undefined)) ??
    (await tryRequest(p, "wallet_getNetwork", {}));
  let network = normalizeNetwork(netRaw, picked.ordinals || picked.payment);

  const address = picked.ordinals;
  if (!network && address) {
    if (/^bc1/i.test(address)) network = "mainnet";
    else if (/^(tb1|2|m|n)/i.test(address)) network = "signet";
    else if (/^bcrt1/i.test(address)) network = "regtest";
  }

  const pay = picked.payment || address;
  const payType = paymentAddressType(pay);

  if (payType === "p2sh-p2wpkh" && !picked.paymentPublicKey) {
    return {
      ok: false,
      message:
        "Connected, but Xverse did not return paymentPublicKey (required for nested 2…/3… payment). Approve wallet_connect for Payment + Ordinals, then reconnect.",
      address,
      paymentAddress: pay,
      paymentAddressType: payType,
      addresses: picked.addresses,
      network: network || "signet",
      provider: "Xverse",
    };
  }

  if (!address) {
    return {
      ok: true,
      message: "Connected, but no address returned — paste/select manually if needed",
      network,
      provider: "Xverse",
      addresses: picked.addresses,
      paymentAddress: pay,
      paymentPublicKey: picked.paymentPublicKey,
      ordinalsPublicKey: picked.ordinalsPublicKey,
      paymentAddressType: payType,
    };
  }

  return {
    ok: true,
    message: "Connected",
    address,
    paymentAddress: pay,
    paymentPublicKey: picked.paymentPublicKey,
    ordinalsPublicKey: picked.ordinalsPublicKey,
    paymentAddressType: payType,
    addresses: picked.addresses,
    network: network || "signet",
    provider: "Xverse",
  };
}

export async function refreshWalletInfo(): Promise<WalletSession> {
  return connectWallet();
}

export async function getWalletUtxos(
  address: string,
  network?: string
): Promise<{
  ok: boolean;
  utxos: WalletUtxo[];
  message: string;
}> {
  // Xverse Sats Connect does not expose getUtxos to dapps — use our indexer proxy.
  try {
    const q = new URLSearchParams({ address });
    if (network) q.set("network", network);
    const r = await fetch(`/api/utxo/for-address?${q}`);
    const data = (await r.json()) as {
      ok?: boolean;
      error?: string;
      utxos?: WalletUtxo[];
      message?: string;
    };
    if (!r.ok || data.error) {
      return { ok: false, utxos: [], message: data.error || `UTXO fetch failed (${r.status})` };
    }
    const utxos = Array.isArray(data.utxos) ? data.utxos : [];
    return {
      ok: utxos.length > 0,
      utxos,
      message: data.message || (utxos.length ? `Found ${utxos.length} UTXO(s)` : "No UTXOs on this address"),
    };
  } catch (e) {
    return { ok: false, utxos: [], message: String(e) };
  }
}

/** Pick the smallest UTXO that can cover commit + fee sized for payment script type. */
export function pickFundingUtxo(
  utxos: WalletUtxo[],
  commitSats: number,
  feeRate: number,
  paymentAddr?: string
): WalletUtxo | null {
  const kind = paymentAddressType(paymentAddr || utxos[0]?.address);
  // Rough signed input + commit(p2tr) + change output overhead
  const inputVb = kind === "p2sh-p2wpkh" ? 91 : kind === "p2tr" ? 58 : 68;
  const overheadVb = 11 + inputVb + 43 + 32; // header + in + commit out + change out
  const need = commitSats + Math.ceil(overheadVb * Math.max(feeRate, 0.1));
  const fit = utxos.filter((u) => u.value >= need);
  if (fit.length) {
    return [...fit].sort((a, b) => a.value - b.value)[0];
  }
  return utxos[0] || null;
}

export async function signPsbtWithWallet(
  psbtBase64: string,
  opts?: {
    signInputs?: Record<string, number[]>;
    broadcast?: boolean;
    /** BIP341/BIP143 sighash; Xverse currently treats this as required (SIGHASH_ALL = 1). */
    allowedSignHash?: number;
  }
): Promise<{ ok: boolean; psbt?: string; txid?: string; message: string }> {
  const p = getProvider();
  if (!p) return { ok: false, message: "No wallet provider" };
  try {
    const params: Record<string, unknown> = {
      psbt: psbtBase64,
      // Default false: Xverse's own broadcast frequently returns HTTP 400 on signet
      broadcast: Boolean(opts?.broadcast),
      // Required by current Xverse builds when omitted (see sats-connect #174).
      allowedSignHash: opts?.allowedSignHash ?? 1,
    };
    if (opts?.signInputs) params.signInputs = opts.signInputs;

    const res = await p.request("signPsbt", params);

    // Sats Connect / Xverse may return { status: 'error', error } or JSON-RPC error
    // without throwing — treat those as cancel/fail, not "unexpected".
    const o = asRecord(res);
    if (o) {
      if (o.status === "error" || o.error) {
        const err = asRecord(o.error) || o;
        const msg = String(err.message || o.message || "Wallet signing failed");
        const code = Number(err.code);
        // Only 4001 is user-reject. -32000 is a generic RPC failure (often "txn error").
        if (code === 4001 || (/^(user rejected|rejected by user|denied by user)/i.test(msg) && !/txn|transaction|output|script/i.test(msg))) {
          return {
            ok: false,
            message:
              "Signing cancelled in wallet (Xverse returned reject). Confirm the funding PSBT popup — Cancel/Reject aborts Inscribe.",
          };
        }
        if (/status code 400|request failed/i.test(msg)) {
          return {
            ok: false,
            message:
              "Xverse rejected the request (HTTP 400). Usually their broadcast backend — Phechan now signs without wallet broadcast; reconnect and retry.",
          };
        }
        return {
          ok: false,
          message: code ? `Wallet error ${code}: ${msg}` : msg,
        };
      }
      // JSON-RPC envelope: { jsonrpc, error } or { jsonrpc, result }
      if (o.jsonrpc && o.error) {
        const err = asRecord(o.error);
        const msg = String(err?.message || "Wallet signing failed");
        const code = Number(err?.code);
        if (code === 4001 || (/^(user rejected|rejected by user|denied by user)/i.test(msg) && !/txn|transaction|output|script/i.test(msg))) {
          return {
            ok: false,
            message:
              "Signing cancelled in wallet (Xverse returned reject). Confirm the funding PSBT popup — Cancel/Reject aborts Inscribe.",
          };
        }
        if (/status code 400|request failed/i.test(msg)) {
          return {
            ok: false,
            message:
              "Xverse HTTP 400 (often wallet broadcast). Retry — we sign only and broadcast via your local node.",
          };
        }
        return {
          ok: false,
          message: code ? `Wallet error ${code}: ${msg}` : msg,
        };
      }
    }

    let signed: string | undefined;
    let txid: string | undefined;
    if (typeof res === "string") {
      if (/^[0-9a-fA-F]{64}$/.test(res)) txid = res;
      else signed = res;
    } else if (o) {
      const result = asRecord(o.result) || o;
      signed =
        (typeof result?.psbt === "string" && result.psbt) ||
        (typeof o.psbt === "string" && o.psbt) ||
        undefined;
      txid =
        (typeof result?.txid === "string" && result.txid) ||
        (typeof o.txid === "string" && o.txid) ||
        undefined;
    }
    if (!signed && !txid) {
      return { ok: false, message: `Unexpected signPsbt response: ${JSON.stringify(res)}` };
    }
    return {
      ok: true,
      psbt: signed,
      txid,
      message: txid ? "Signed & broadcast" : "Signed in wallet",
    };
  } catch (e) {
    const msg = String(e);
    if (
      /user rejected|rejected by user|denied by user|4001/i.test(msg) &&
      !/txn|transaction|output|script/i.test(msg)
    ) {
      return {
        ok: false,
        message:
          "Signing cancelled in wallet. Confirm the funding PSBT in Xverse to continue.",
      };
    }
    if (/status code 400|request failed/i.test(msg)) {
      return {
        ok: false,
        message:
          "Xverse HTTP 400 during sign/broadcast. Phechan signs with broadcast=false and pushes via local node — hard-refresh and retry.",
      };
    }
    return { ok: false, message: msg };
  }
}

/** @deprecated Prefer funding PSBT — kept as fallback */
export async function sendTransfer(opts: {
  recipient: string;
  amountSats: number;
}): Promise<{ ok: boolean; txid?: string; message: string }> {
  const p = getProvider();
  if (!p) return { ok: false, message: "No wallet provider" };
  try {
    const params = {
      recipients: [{ address: opts.recipient, amount: opts.amountSats }],
    };
    const res =
      (await tryRequest(p, "sendTransfer", params)) ??
      (await tryRequest(p, "btc_sendTransfer", params));
    if (!res) {
      return { ok: false, message: "sendTransfer failed or was cancelled" };
    }
    const o = asRecord(res);
    const result = o?.result !== undefined ? asRecord(o.result) : o;
    const txid =
      (typeof result?.txid === "string" && result.txid) ||
      (typeof o?.txid === "string" && o.txid) ||
      (typeof res === "string" ? res : undefined);
    if (!txid || txid.length < 64) {
      return {
        ok: false,
        message: `Transfer submitted but no txid in response: ${JSON.stringify(res)}`,
      };
    }
    return { ok: true, txid, message: "Commit funded" };
  } catch (e) {
    return { ok: false, message: String(e) };
  }
}

export function explorerTxUrl(network: string, txid: string): string {
  const n = network.toLowerCase();
  // Prefer memepool on signet — mempool.space often times out from this network
  if (n === "mainnet" || n === "bitcoin") return `https://mempool.space/tx/${txid}`;
  if (n === "signet") return `https://memepool.space/signet/tx/${txid}`;
  if (n === "testnet") return `https://mempool.space/testnet/tx/${txid}`;
  return `txid:${txid}`;
}
