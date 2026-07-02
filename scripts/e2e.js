/** End-to-end test on local validator: initialize -> create_token -> buy -> sell -> crank. 
 *  Uses the SAME hand-built instructions as the frontend, validating both. */
const w3 = require("@solana/web3.js"); const fs = require("fs"); const crypto = require("crypto");
const PROGRAM_ID = new w3.PublicKey("AoVUouTT7TqwruCcseNe6BSETKkDV5mvcjaUbN83B8h6");
const TOKEN22 = new w3.PublicKey("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
const ATA_PROG = new w3.PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const disc = n => crypto.createHash("sha256").update("global:" + n).digest().subarray(0, 8);
const u16 = n => { const b = Buffer.alloc(2); b.writeUInt16LE(n); return b; };
const u32 = n => { const b = Buffer.alloc(4); b.writeUInt32LE(n); return b; };
const u64 = n => { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(n)); return b; };
const i64 = n => { const b = Buffer.alloc(8); b.writeBigInt64LE(BigInt(n)); return b; };
const str = s => Buffer.concat([u32(Buffer.byteLength(s)), Buffer.from(s)]);
const kp = p => w3.Keypair.fromSecretKey(new Uint8Array(JSON.parse(fs.readFileSync(p))));
(async () => {
  const conn = new w3.Connection("http://127.0.0.1:8899", "confirmed");
  const admin = kp(process.env.HOME + "/.config/solana/id.json");
  const attestor = kp("/home/claude/liftoff/attestor-devnet.json");
  const [config] = w3.PublicKey.findProgramAddressSync([Buffer.from("config")], PROGRAM_ID);
  const send = (ixs, signers) => w3.sendAndConfirmTransaction(conn, new w3.Transaction().add(...ixs), signers);

  // 1. initialize
  let data = Buffer.concat([disc("initialize"), attestor.publicKey.toBuffer(), admin.publicKey.toBuffer(),
    u16(100), u16(5), u64(85n * 10n ** 9n)]);
  await send([new w3.TransactionInstruction({ programId: PROGRAM_ID, data, keys: [
    { pubkey: admin.publicKey, isSigner: true, isWritable: true },
    { pubkey: config, isSigner: false, isWritable: true },
    { pubkey: w3.SystemProgram.programId, isSigner: false, isWritable: false }]})], [admin]);
  console.log("1. initialize OK — config", config.toBase58());

  // 2. create_token (tier 1, 5000% rate, 30 SOL threshold, no delay, Degen burn)
  const mint = w3.Keypair.generate();
  const [curve] = w3.PublicKey.findProgramAddressSync([Buffer.from("curve"), mint.publicKey.toBuffer()], PROGRAM_ID);
  const ata = o => w3.PublicKey.findProgramAddressSync([o.toBuffer(), TOKEN22.toBuffer(), mint.publicKey.toBuffer()], ATA_PROG)[0];
  data = Buffer.concat([disc("create_token"), str("Test Monster"), str("TSTL1ft"), str("https://x/t.json"),
    Buffer.from([1]), u32(500000), u64(30n * 10n ** 9n), i64(0), u16(9900)]);
  await send([new w3.TransactionInstruction({ programId: PROGRAM_ID, data, keys: [
    { pubkey: admin.publicKey, isSigner: true, isWritable: true },
    { pubkey: attestor.publicKey, isSigner: true, isWritable: false },
    { pubkey: config, isSigner: false, isWritable: false },
    { pubkey: mint.publicKey, isSigner: true, isWritable: true },
    { pubkey: curve, isSigner: false, isWritable: true },
    { pubkey: ata(curve), isSigner: false, isWritable: true },
    { pubkey: TOKEN22, isSigner: false, isWritable: false },
    { pubkey: ATA_PROG, isSigner: false, isWritable: false },
    { pubkey: w3.SystemProgram.programId, isSigner: false, isWritable: false }]})], [admin, mint, attestor]);
  console.log("2. create_token OK — mint", mint.publicKey.toBase58());

  // tier violation must FAIL: tier 0 with 200% rate
  const mint2 = w3.Keypair.generate();
  const [curve2] = w3.PublicKey.findProgramAddressSync([Buffer.from("curve"), mint2.publicKey.toBuffer()], PROGRAM_ID);
  const ata2 = w3.PublicKey.findProgramAddressSync([curve2.toBuffer(), TOKEN22.toBuffer(), mint2.publicKey.toBuffer()], ATA_PROG)[0];
  data = Buffer.concat([disc("create_token"), str("Cheater"), str("HAXL1ft"), str("https://x/h.json"),
    Buffer.from([0]), u32(20000), u64(30n * 10n ** 9n), i64(0), u16(0)]);
  try {
    await send([new w3.TransactionInstruction({ programId: PROGRAM_ID, data, keys: [
      { pubkey: admin.publicKey, isSigner: true, isWritable: true },
      { pubkey: attestor.publicKey, isSigner: true, isWritable: false },
      { pubkey: config, isSigner: false, isWritable: false },
      { pubkey: mint2.publicKey, isSigner: true, isWritable: true },
      { pubkey: curve2, isSigner: false, isWritable: true },
      { pubkey: ata2, isSigner: false, isWritable: true },
      { pubkey: TOKEN22, isSigner: false, isWritable: false },
      { pubkey: ATA_PROG, isSigner: false, isWritable: false },
      { pubkey: w3.SystemProgram.programId, isSigner: false, isWritable: false }]})], [admin, mint2, attestor]);
    console.log("3. TIER CAP FAILED TO ENFORCE — BUG!");
  } catch { console.log("3. tier cap enforced OK (no-NFT capped at 100%)"); }

  // 4. buy 2 SOL
  const tradeKeys = [
    { pubkey: admin.publicKey, isSigner: true, isWritable: true },
    { pubkey: config, isSigner: false, isWritable: false },
    { pubkey: mint.publicKey, isSigner: false, isWritable: false },
    { pubkey: curve, isSigner: false, isWritable: true },
    { pubkey: ata(curve), isSigner: false, isWritable: true },
    { pubkey: ata(admin.publicKey), isSigner: false, isWritable: true },
    { pubkey: admin.publicKey, isSigner: false, isWritable: true },
    { pubkey: admin.publicKey, isSigner: false, isWritable: true },
    { pubkey: TOKEN22, isSigner: false, isWritable: false },
    { pubkey: ATA_PROG, isSigner: false, isWritable: false },
    { pubkey: w3.SystemProgram.programId, isSigner: false, isWritable: false }];
  data = Buffer.concat([disc("buy"), u64(2n * 10n ** 9n), u64(0)]);
  await send([new w3.TransactionInstruction({ programId: PROGRAM_ID, data, keys: tradeKeys })], [admin]);
  let bal = await conn.getTokenAccountBalance(ata(admin.publicKey));
  console.log("4. buy OK — raw balance:", bal.value.uiAmountString);

  // 5. sell 25%
  data = Buffer.concat([disc("sell"), u64(Math.floor(+bal.value.amount * 0.25)), u64(0)]);
  await send([new w3.TransactionInstruction({ programId: PROGRAM_ID, data, keys: tradeKeys })], [admin]);
  bal = await conn.getTokenAccountBalance(ata(admin.publicKey));
  console.log("5. sell OK — raw balance:", bal.value.uiAmountString);

  // 6. crank too soon must fail
  const crankKeys = [
    { pubkey: admin.publicKey, isSigner: true, isWritable: false },
    { pubkey: mint.publicKey, isSigner: false, isWritable: true },
    { pubkey: curve, isSigner: false, isWritable: true },
    { pubkey: TOKEN22, isSigner: false, isWritable: false }];
  try {
    await send([new w3.TransactionInstruction({ programId: PROGRAM_ID, data: disc("crank"), keys: crankKeys })], [admin]);
    console.log("6. CRANK MIN INTERVAL NOT ENFORCED — BUG!");
  } catch { console.log("6. crank-too-soon rejected OK (5 min interval)"); }

  fs.writeFileSync("/tmp/e2e-state.json", JSON.stringify({ mint: mint.publicKey.toBase58() }));
  console.log("E2E PART 1 COMPLETE");
})().catch(e => { console.error("E2E FAILED:", e.message); if (e.logs) console.error(e.logs.slice(-8).join("\n")); process.exit(1); });
