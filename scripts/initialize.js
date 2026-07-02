/** Post-deploy: create the Config PDA. Run once from the deployer wallet. */
const w3 = require("@solana/web3.js");
const fs = require("fs");
const PROGRAM_ID = new w3.PublicKey("AoVUouTT7TqwruCcseNe6BSETKkDV5mvcjaUbN83B8h6");
const ATTESTOR = new w3.PublicKey("6Xm1yGGyeZtVAo3NEJJnt6Eu9tzAw8e5Mo53kvQVy1m8");
const FEE_RECIPIENT = new w3.PublicKey("9Z4HpvTp6hwsc66hCPnN86EGKQqNWk3mHWwJQEdbD3Cz");
const FEE_BPS = 100, CREATOR_FEE_BPS = 5, GRAD_DEFAULT = 85n * 1000000000n;
(async () => {
  const admin = w3.Keypair.fromSecretKey(new Uint8Array(JSON.parse(fs.readFileSync(process.env.HOME + "/.config/solana/id.json"))));
  const conn = new w3.Connection("https://api.devnet.solana.com", "confirmed");
  const [config] = w3.PublicKey.findProgramAddressSync([Buffer.from("config")], PROGRAM_ID);
  const data = Buffer.concat([
    Buffer.from([175,175,109,31,13,152,155,237]),
    ATTESTOR.toBuffer(), FEE_RECIPIENT.toBuffer(),
    Buffer.from(new Uint8Array(new Uint16Array([FEE_BPS]).buffer)),
    Buffer.from(new Uint8Array(new Uint16Array([CREATOR_FEE_BPS]).buffer)),
    (b => { b.writeBigUInt64LE(GRAD_DEFAULT); return b; })(Buffer.alloc(8)),
  ]);
  const ix = new w3.TransactionInstruction({ programId: PROGRAM_ID, data, keys: [
    { pubkey: admin.publicKey, isSigner: true, isWritable: true },
    { pubkey: config, isSigner: false, isWritable: true },
    { pubkey: w3.SystemProgram.programId, isSigner: false, isWritable: false },
  ]});
  const sig = await w3.sendAndConfirmTransaction(conn, new w3.Transaction().add(ix), [admin]);
  console.log("config initialized:", config.toBase58(), "sig:", sig);
})();
