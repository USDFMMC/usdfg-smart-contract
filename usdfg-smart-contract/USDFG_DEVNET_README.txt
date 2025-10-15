USDFG Smart Contract – Devnet Integration Info
==============================================

Hey DEV,

Here's everything you need to integrate the USDFG smart contract with the frontend on devnet.  
**No private keys or wallet seeds are included or needed.**

---

Program ID:
-----------
2KL4BKvUtDmABvuvRopkCEb33myWM1W9BGodAZ82RWDT

USDFG Mint Address:
-------------------
[INSERT_YOUR_USDFG_MINT_ADDRESS_HERE]

IDL File:
---------
See attached `usdfg_smart_contract.json` in the zip package.

---

PDA Derivation Seeds
--------------------
You can use these with `PublicKey.findProgramAddress` in web3.js or Anchor.

- Admin State PDA:
  [Buffer.from("admin")]

- Price Oracle PDA:
  [Buffer.from("price_oracle")]

- Challenge PDA:
  [
    Buffer.from("challenge"),
    creatorPublicKey.toBuffer(),
    challengeSeedPublicKey.toBuffer()
  ]
  - `creatorPublicKey`: The public key of the challenge creator
  - `challengeSeedPublicKey`: The public key of the unique challenge seed (a Keypair per challenge)

- Escrow Wallet PDA:
  [Buffer.from("escrow_wallet")]

- Escrow Token Account PDA:
  [
    Buffer.from("escrow_wallet"),
    challengePDA.toBuffer(),
    mintPublicKey.toBuffer()
  ]
  - `challengePDA`: The address derived above
  - `mintPublicKey`: The USDFG mint address

---

How to Use:
-----------
- Use the above seeds and the program ID with `findProgramAddress` to derive all necessary addresses in the frontend.
- No private keys or wallet seeds are needed or shared.

Let me know if you need any more info or help with integration! 