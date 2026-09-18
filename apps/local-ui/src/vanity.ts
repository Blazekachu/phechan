/** Vanity TXID estimate — mirrors runes-etch VanityGrinder.estimateDifficulty. */

export const VANITY_HEX_RE = /^[0-9a-f]*$/;
export const MAX_VANITY_TOTAL = 6;

export function sanitizeVanityHex(raw: string, maxLen: number): string {
  return raw
    .toLowerCase()
    .replace(/[^0-9a-f]/g, "")
    .slice(0, Math.max(0, maxLen));
}

export function estimateVanity(
  prefix: string,
  suffix: string,
  hashesPerSec = 50_000
): { avgAttempts: number; description: string; eta: string } {
  const totalChars = prefix.length + suffix.length;
  const avgAttempts = Math.pow(16, totalChars) || 1;

  let description: string;
  if (avgAttempts <= 1_000) description = "Instant (< 1 second)";
  else if (avgAttempts <= 100_000) description = "Fast (a few seconds)";
  else if (avgAttempts <= 10_000_000) description = "Moderate (seconds to a minute)";
  else if (avgAttempts <= 1_000_000_000) description = "Slow (minutes)";
  else description = "Very slow (could take hours)";

  const seconds = avgAttempts / hashesPerSec;
  let eta: string;
  if (seconds < 1) eta = "< 1s";
  else if (seconds < 60) eta = `~${Math.ceil(seconds)}s`;
  else if (seconds < 3600) eta = `~${Math.ceil(seconds / 60)} min`;
  else eta = `~${(seconds / 3600).toFixed(1)} h`;

  return { avgAttempts, description, eta };
}
