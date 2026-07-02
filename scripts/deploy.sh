#!/bin/bash
# One-shot devnet deploy: run when the deployer wallet has ~3.5 SOL.
set -e
export PATH="/root/.local/share/solana/install/active_release/bin:$HOME/.local/bin:$PATH"
cd "$(dirname "$0")/.."
solana balance
anchor deploy --provider.cluster devnet
cd scripts && npm init -y >/dev/null 2>&1; npm i @solana/web3.js@1.95.3 >/dev/null 2>&1
node initialize.js
# Hand upgrade authority to the user's wallet:
solana program set-upgrade-authority AoVUouTT7TqwruCcseNe6BSETKkDV5mvcjaUbN83B8h6 \
  --new-upgrade-authority 9Z4HpvTp6hwsc66hCPnN86EGKQqNWk3mHWwJQEdbD3Cz --skip-new-upgrade-authority-signer-check
echo "DEPLOYED + INITIALIZED + upgrade authority transferred"
