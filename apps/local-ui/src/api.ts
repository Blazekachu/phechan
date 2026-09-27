export type CliResponse = {
  ok?: boolean;
  error?: string;
  fields?: Record<string, string | string[]>;
  raw?: string[];
  stderr?: string;
  [key: string]: unknown;
};

export type InscribeMode = "text" | "delegate" | "upload";

export type InscribePlan = {
  mode: InscribeMode;
  network: string;
  body?: string;
  contentBase64?: string;
  contentType?: string;
  fileName?: string;
  delegateId?: string;
  title?: string;
  metadata?: string;
  metaprotocol?: string;
  /** Brotli body + content_encoding tag 9 = br */
  compressBr?: boolean;
  parentId?: string;
  /** txid:vout of parent inscription UTXO (from Verify) */
  parentOutpoint?: string;
  parentValue?: number;
  parentAddress?: string;
  parentScriptHex?: string;
  /**
   * Child on the parent's own sat: fund commit from parent UTXO, single-input reveal
   * with tag 3 (no FI/FO). Inferred in UI when parent + sat target resolve to same outpoint.
   */
  sameSatParent?: boolean;
  /** Where parent sat returns (vout0) — usually ordinals address */
  vaultAddress?: string;
  /** Ordinals / taproot x-only pubkey for parent input signing */
  ordinalsPublicKey?: string;
  /** Auto-detected: digits = sat number, else inscription id / satpoint */
  satTarget?: string;
  opReturn?: string;
  vanityPrefix?: string;
  vanitySuffix?: string;
  /** Commit funding TXID vanity (grind funding PSBT locktime) */
  commitVanityPrefix?: string;
  commitVanitySuffix?: string;
  /**
   * Legacy single rate — prefer commitFeeRate + revealFeeRate.
   * Kept so older interrupted commits / bundles still load.
   */
  feeRate?: number;
  /** Fee rate for the commit funding PSBT only (sats/vB). */
  commitFeeRate?: number;
  /**
   * Fee rate used to size commit_sats = postage + reveal fee (sats/vB).
   * After funding, actual reveal fee is commit_value − postage.
   */
  revealFeeRate?: number;
  /** Inscription output value (postage / padding), sats */
  postage?: number;
  /** Ordinals / receive address for reveal output */
  destination?: string;
  /** Payment address (change + signInputs) */
  paymentAddress?: string;
  /** Payment pubkey hex — required when payment is nested P2SH-P2WPKH */
  paymentPublicKey?: string;
  /** Selected funding UTXO */
  fundingTxid?: string;
  fundingVout?: number;
  fundingValue?: number;
  fundingScriptHex?: string;
  fundingAddress?: string;
  /** Wallet-funded commit tx */
  commitTxid?: string;
  commitVout?: number;
  commitValue?: number;
  /** Auto-attached after disclosure on mainnet (not typed by user) */
  confirm?: string;
};

export type PrepareResult = CliResponse & {
  commit_address?: string;
  commit_sats?: string;
  postage_sats?: string;
  reveal_fee_sats?: string;
  commit_fee_estimate_sats?: string;
  network_fee_sats?: string;
  fee_rate_sats_vb?: string;
  reveal_vsize?: string;
};

export type FundPsbtResult = CliResponse & {
  psbt_base64?: string;
  commit_funding_fee_sats?: string;
  commit_vanity_txid?: string;
  commit_sats?: string;
};

export type RevealResult = CliResponse & {
  reveal_txid?: string;
  commit_txid?: string;
  inscription_id_guess?: string;
  vanity_reveal_txid?: string;
  psbt_base64?: string;
  parent_lands?: string;
  child_lands?: string;
  vault_address?: string;
  parent_outpoint?: string;
};

export type VerifyResult = {
  ok: boolean;
  owned?: boolean;
  error?: string;
  detail?: string;
  txid?: string;
  vout?: number;
  value?: number;
  address?: string;
  satNumber?: number | null;
  inscriptionIds?: string[];
  reinscribe?: boolean;
  contentType?: string;
};

async function post(path: string, body: Record<string, unknown>): Promise<CliResponse & VerifyResult> {
  const res = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return res.json();
}

export const api = {
  health: () => fetch("/api/health").then((r) => r.json()),
  prepareInscription: (plan: InscribePlan) =>
    post("/api/inscription/prepare", plan as unknown as Record<string, unknown>) as Promise<
      PrepareResult
    >,
  brotliPreview: (opts: {
    body?: string;
    contentBase64?: string;
    feeRate?: number;
    revealFeeRate?: number;
  }) =>
    post("/api/inscription/brotli-preview", {
      ...opts,
      feeRate: opts.revealFeeRate ?? opts.feeRate,
    }),
  fundCommitPsbt: (plan: InscribePlan & Record<string, unknown>) =>
    post("/api/inscription/fund-psbt", plan) as Promise<FundPsbtResult>,
  revealInscription: (plan: InscribePlan) =>
    post("/api/inscription/reveal", plan as unknown as Record<string, unknown>) as Promise<
      RevealResult
    >,
  inspectPsbt: (opts: { base64: string; network?: string }) =>
    post("/api/psbt/inspect", opts) as Promise<
      CliResponse & { vin?: string[]; vout?: string[]; fee_sats?: string; unsigned_txid?: string }
    >,
  finalizeFundingPsbt: (opts: {
    base64: string;
    network?: string;
    broadcast?: boolean;
    paymentPublicKey?: string;
    confirm?: string;
  }) => post("/api/psbt/finalize-funding", opts),
  /** Atomic commit+reveal via bitcoind submitpackage (Esplora sequential fallback). */
  submitPackage: (opts: {
    network: string;
    commitHex: string;
    revealHex: string;
    confirm?: string;
  }) =>
    post("/api/tx/submit-package", opts as unknown as Record<string, unknown>) as Promise<
      CliResponse & {
        package_via?: string;
        package_note?: string;
        txids?: string[];
      }
    >,
  previewInscription: (plan: InscribePlan) =>
    post("/api/inscription/preview", plan as unknown as Record<string, unknown>),
  exportPsbt: (plan: InscribePlan) =>
    post("/api/inscription/export-psbt", plan as unknown as Record<string, unknown>),
  finalizePsbt: (opts: {
    base64: string;
    network?: string;
    expectBody?: string;
    broadcast?: boolean;
    confirm?: string;
  }) => post("/api/psbt/finalize-import", opts),
  verifyParent: (id: string, address: string, network: string) =>
    post("/api/verify/parent", { id, address, network }),
  verifySatTarget: (input: string, address: string, network: string) =>
    post("/api/verify/sat-target", { input, address, network }),
  verifyDelegate: (id: string, network: string) =>
    post("/api/verify/delegate", { id, network }),
};
