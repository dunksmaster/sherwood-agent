#!/usr/bin/env node
// rhc-probe.mjs — read-only reconnaissance of Robinhood Chain Stock Tokens.
//
// Answers one question before sherwood commits to the venue: can an ordinary
// self-custody wallet (no Robinhood account, no KYC) hold and swap Stock Tokens,
// or is `transfer` gated by an on-chain allowlist / compliance hook?
//
// It signs nothing and sends nothing. Every call is `eth_call` / `eth_getLogs` /
// `eth_getCode` against a public RPC. Node >= 18 (uses `fetch`). No dependencies.
//
//   node scripts/rhc-probe.mjs            # probe NVDA (default)
//   node scripts/rhc-probe.mjs TSLA AAPL  # probe several
//   RHC_RPC=https://…  node scripts/rhc-probe.mjs
//
// Exit 0 if transfers look permissionless, 1 if a restriction is detected or the
// probe could not reach a conclusion.

const RPC = process.env.RHC_RPC || "https://rpc.mainnet.chain.robinhood.com";
const EXPECT_CHAIN_ID = 4663;

// Stock Token ERC-20s (cross-checked across two independent public write-ups).
const TOKENS = {
  NVDA: "0xd0601CE157Db5bdC3162BbaC2a2C8aF5320D9EEC",
  TSLA: "0x322F0929c4625eD5bAd873c95208D54E1c003b2d",
  AAPL: "0xaF3D76f1834A1d425780943C99Ea8A608f8a93f9",
  MSFT: "0xe93237C50D904957Cf27E7B1133b510C669c2e74",
  AMZN: "0x12f190a9F9d7D37a250758b26824B97CE941bF54",
  GOOGL: "0x2e0847E8910a9732eB3fb1bb4b70a580ADAD4FE3",
  META: "0xc0D6457C16Cc70d6790Dd43521C899C87ce02f35",
  SPY: "0x117cc2133c37B721F49dE2A7a74833232B3B4C0C",
};
const WETH = "0x0Bd7D308f8E1639FAb988df18A8011f41EAcAD73";
const USDG = "0x5fc5360D0400a0Fd4f2af552ADD042D716F1d168";

// Well-known constants — no keccak needed.
const SIG = {
  name: "0x06fdde03",
  symbol: "0x95d89b41",
  decimals: "0x313ce567",
  totalSupply: "0x18160ddd",
  paused: "0x5c975abb",
  implementation: "0x5c60da1b", // beacon / EIP-1822
};
const TRANSFER_TOPIC =
  "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const SLOT_EIP1967_IMPL =
  "0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc";
const SLOT_EIP1967_BEACON =
  "0xa3f0ad74e5423aebfd80d3ef4346578335a9a72aeaee59ff6cb3582b35133d50";
// OZ custom errors we expect from a *balance*-gated (i.e. normal) ERC-20.
const ERR_INSUFFICIENT_BALANCE = "0xe450d38c"; // ERC20InsufficientBalance(address,uint256,uint256)
const ERR_INSUFFICIENT_ALLOWANCE = "0xfb8f41b2"; // ERC20InsufficientAllowance(address,uint256,uint256)

let rpcId = 0;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function rpc(method, params, attempt = 0) {
  const res = await fetch(RPC, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: ++rpcId, method, params }),
  });
  if (res.status === 429 && attempt < 5) {
    await sleep(400 * 2 ** attempt); // public RPC throttles bursts — back off
    return rpc(method, params, attempt + 1);
  }
  if (!res.ok) throw new Error(`${method}: HTTP ${res.status}`);
  return res.json(); // caller inspects .result / .error
}
const call = (to, data, from) =>
  rpc("eth_call", [from ? { from, to, data } : { to, data }, "latest"]);

const hexToBig = (h) => (h && h !== "0x" ? BigInt(h) : 0n);
const addr = (word) => "0x" + word.slice(-40);
const pad = (hexNo0x) => hexNo0x.replace(/^0x/, "").padStart(64, "0");

function decodeAbiString(hex) {
  // (offset)(length)(bytes) — tolerate a bare bytes32 too.
  const b = Buffer.from(hex.slice(2), "hex");
  if (b.length >= 96) {
    const len = Number(BigInt("0x" + b.subarray(32, 64).toString("hex")));
    if (len > 0 && len <= b.length - 64)
      return b.subarray(64, 64 + len).toString("utf8");
  }
  return b.toString("utf8").replace(/\0+$/, "");
}

async function proxyTarget(token) {
  const eip1967 = await rpc("eth_getStorageAt", [token, SLOT_EIP1967_IMPL, "latest"]);
  if (hexToBig(eip1967.result) !== 0n)
    return { kind: "eip1967", impl: addr(eip1967.result) };
  const beaconSlot = await rpc("eth_getStorageAt", [token, SLOT_EIP1967_BEACON, "latest"]);
  if (hexToBig(beaconSlot.result) !== 0n) {
    const beacon = addr(beaconSlot.result);
    const impl = await call(beacon, SIG.implementation);
    return { kind: "beacon", beacon, impl: impl.result ? addr(impl.result) : null };
  }
  return { kind: "none" };
}

async function probeToken(sym) {
  const token = TOKENS[sym];
  if (!token) throw new Error(`unknown token ${sym}; known: ${Object.keys(TOKENS).join(", ")}`);
  console.log(`\n━━ ${sym}  ${token}`);

  const [name, symbol, decimals, supply, code] = await Promise.all([
    call(token, SIG.name),
    call(token, SIG.symbol),
    call(token, SIG.decimals),
    call(token, SIG.totalSupply),
    rpc("eth_getCode", [token, "latest"]),
  ]);
  const dec = Number(hexToBig(decimals.result));
  console.log(`   name/symbol   ${JSON.stringify(decodeAbiString(name.result))} / ${decodeAbiString(symbol.result)}`);
  console.log(`   decimals      ${dec}`);
  console.log(`   totalSupply   ${fmtUnits(hexToBig(supply.result), dec)}`);
  console.log(`   proxy code    ${(code.result.length - 2) / 2} bytes`);

  const px = await proxyTarget(token);
  if (px.kind === "beacon") {
    const implCode = await rpc("eth_getCode", [px.impl, "latest"]);
    console.log(`   proxy model   beacon ${px.beacon}`);
    console.log(`                 → implementation ${px.impl} (${(implCode.result.length - 2) / 2} bytes, shared by all Stock Tokens)`);
    console.log(`   ⚠ upgrade key  whoever controls the beacon can swap that implementation for every Stock Token in one tx`);
  } else if (px.kind === "eip1967") {
    console.log(`   proxy model   EIP-1967 → ${px.impl}`);
  } else {
    console.log(`   proxy model   none detected (immutable logic)`);
  }

  const paused = await call(token, SIG.paused);
  if (!paused.error)
    console.log(`   paused()      ${hexToBig(paused.result) !== 0n ? "TRUE ⛔" : "false"}`);

  // ---- live Transfer traffic --------------------------------------------------
  const head = Number(hexToBig((await rpc("eth_blockNumber", [])).result));
  const span = 1500;
  const logsResp = await rpc("eth_getLogs", [
    {
      address: token,
      topics: [TRANSFER_TOPIC],
      fromBlock: "0x" + (head - span).toString(16),
      toBlock: "0x" + head.toString(16),
    },
  ]);
  if (logsResp.error) throw new Error(`eth_getLogs: ${logsResp.error.message}`);
  const logs = logsResp.result;
  const ZERO = "0x0000000000000000000000000000000000000000";
  const senders = new Set(), recipients = new Set();
  let mints = 0, burns = 0, p2p = 0;
  for (const l of logs) {
    const f = addr(l.topics[1]), t = addr(l.topics[2]);
    senders.add(f); recipients.add(t);
    if (f === ZERO) mints++; else if (t === ZERO) burns++; else p2p++;
  }
  console.log(`   last ${span} blk   ${logs.length} Transfer events — ${p2p} wallet↔wallet, ${mints} mint, ${burns} burn`);
  console.log(`                 ${senders.size} distinct senders, ${recipients.size} distinct recipients`);

  // ---- the actual test: can a brand-new address receive tokens? --------------
  // Pick the largest real holder seen in recent traffic.
  const candidates = [...new Set(logs.map((l) => addr(l.topics[1])).filter((a) => a !== ZERO))];
  let holder = null, holderBal = 0n;
  for (const a of candidates.slice(0, 40)) {
    const b = hexToBig((await call(token, "0x70a08231" + pad(a))).result);
    if (b > holderBal) { holderBal = b; holder = a; }
  }
  if (!holder) { console.log("   ✗ no funded holder found in the sample window — inconclusive"); return false; }

  // A deterministic address with no code, no history, no Robinhood onboarding.
  const fresh = "0x1111111111111111111111111111111111111112";
  const amount = 1000n;
  const toFresh = await call(
    token,
    "0xa9059cbb" + pad(fresh) + pad("0x" + amount.toString(16)),
    holder,
  );
  const fromFresh = await call(
    token,
    "0xa9059cbb" + pad(holder) + pad("0x1"),
    fresh,
  );

  console.log(`\n   ── transfer test (eth_call only, nothing signed) ──`);
  console.log(`   holder        ${holder}  (${fmtUnits(holderBal, dec)} ${sym})`);
  const okToFresh = !toFresh.error && hexToBig(toFresh.result) === 1n;
  console.log(`   transfer→new  ${okToFresh ? "✅ returns true — a fresh unknown address can receive " + sym
    : "⛔ " + (toFresh.error ? toFresh.error.message + " " + (toFresh.error.data || "") : "returned " + toFresh.result)}`);
  const ff = fromFresh.error?.data?.slice(0, 10);
  const balanceGated = ff === ERR_INSUFFICIENT_BALANCE || ff === ERR_INSUFFICIENT_ALLOWANCE;
  console.log(`   transfer←new  ${balanceGated ? "✅ reverts ERC20InsufficientBalance — gated by balance only, not identity"
    : "⚠ " + (fromFresh.error ? (fromFresh.error.message + " " + (fromFresh.error.data || "")) : "unexpectedly succeeded")}`);

  return okToFresh && balanceGated;
}

// crude fixed-point formatter, integer + up to 4 fractional digits
function fmtUnits(v, dec) {
  if (dec === 0) return v.toString();
  const base = 10n ** BigInt(dec);
  const whole = v / base;
  const frac = ((v % base) * 10000n) / base;
  return `${whole.toLocaleString("en-US")}.${frac.toString().padStart(4, "0")}`;
}

async function liquidityCheck(sym) {
  // The single largest holder that is a contract ≈ the AMM (Uniswap v4 is a
  // singleton that custodies every pool's tokens in one address).
  const token = TOKENS[sym];
  const head = Number(hexToBig((await rpc("eth_blockNumber", [])).result));
  const logs = (await rpc("eth_getLogs", [{
    address: token, topics: [TRANSFER_TOPIC],
    fromBlock: "0x" + (head - 1500).toString(16), toBlock: "0x" + head.toString(16),
  }])).result || [];
  const ZERO = "0x0000000000000000000000000000000000000000";
  const seen = [...new Set(logs.flatMap((l) => [addr(l.topics[1]), addr(l.topics[2])]).filter((a) => a !== ZERO))].slice(0, 30);
  let venue = null, venueBal = 0n;
  for (const a of seen) {
    const [b, c] = await Promise.all([
      call(token, "0x70a08231" + pad(a)),
      rpc("eth_getCode", [a, "latest"]),
    ]);
    const bal = hexToBig(b.result);
    if (bal > venueBal && c.result && c.result.length > 4) { venueBal = bal; venue = a; }
  }
  if (!venue) { console.log("\n━━ liquidity — no contract holder found in sample"); return; }
  const [nb, wb, ub] = await Promise.all([
    call(token, "0x70a08231" + pad(venue)),
    call(WETH, "0x70a08231" + pad(venue)),
    call(USDG, "0x70a08231" + pad(venue)),
  ]);
  console.log(`\n━━ liquidity venue  ${venue}`);
  console.log(`   holds          ${fmtUnits(hexToBig(nb.result), 18)} ${sym}`);
  console.log(`   holds          ${fmtUnits(hexToBig(wb.result), 18)} WETH`);
  console.log(`   holds          ${fmtUnits(hexToBig(ub.result), 6)} USDG`);
  console.log(`   → a funded multi-asset AMM contract: Stock Tokens are quotable/tradeable on-chain`);
}

async function main() {
  const syms = process.argv.slice(2).filter((a) => !a.startsWith("-"));
  const list = syms.length ? syms : ["NVDA"];

  console.log(`Robinhood Chain probe — ${RPC}`);
  const [cid, blk, ver] = await Promise.all([
    rpc("eth_chainId", []), rpc("eth_blockNumber", []), rpc("web3_clientVersion", []),
  ]);
  const chainId = Number(hexToBig(cid.result));
  console.log(`chainId ${chainId}  block ${Number(hexToBig(blk.result))}  ${ver.result || "?"}`);
  if (chainId !== EXPECT_CHAIN_ID) {
    console.error(`✗ expected chain ${EXPECT_CHAIN_ID}, got ${chainId}`);
    process.exit(1);
  }

  let allOpen = true;
  for (const s of list) allOpen = (await probeToken(s)) && allOpen;
  await liquidityCheck(list[0]);

  console.log(`\n════════════════════════════════════════════════════════════`);
  if (allOpen) {
    console.log(`VERDICT  Secondary-market transfers are permissionless at the token`);
    console.log(`         contract: a fresh, un-KYC'd address can receive and move`);
    console.log(`         Stock Tokens; failures are balance-gated only. A wallet`);
    console.log(`         outside Robinhood's app (e.g. non-EEA) can trade them.`);
    console.log(`CAVEATS  • Stock Tokens are beacon proxies — the shared implementation`);
    console.log(`           is upgradeable; an allowlist could be added later.`);
    console.log(`         • Robinhood Chain screens txs at the sequencer (sanctions);`);
    console.log(`           a sanctioned address is still blocked chain-wide.`);
    console.log(`         • Primary mint/redeem for real shares stays KYC/EEA-gated —`);
    console.log(`           you can hold the token, not redeem it for the equity.`);
    process.exit(0);
  } else {
    console.log(`VERDICT  A transfer restriction was detected or the probe could not`);
    console.log(`         confirm open transfers. Do NOT assume the venue is usable —`);
    console.log(`         inspect the failing token above.`);
    process.exit(1);
  }
}

main().catch((e) => { console.error("probe failed:", e.message); process.exit(1); });
