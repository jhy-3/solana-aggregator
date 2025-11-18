import type { Program } from "@coral-xyz/anchor";
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  createMint,
  getAssociatedTokenAddressSync,
  getOrCreateAssociatedTokenAccount,
  mintTo,
} from "@solana/spl-token";
import {
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SystemProgram,
} from "@solana/web3.js";
import { expect } from "chai";
import path from "path";
import { Vault } from "../../target/types/vault";

const workspaceRoot = path.resolve(__dirname, "..", "..");
const anchorToml = path.join(workspaceRoot, "Anchor.toml");
process.env.ANCHOR_CONFIG = process.env.ANCHOR_CONFIG ?? anchorToml;
const defaultWallet = path.join(workspaceRoot, "target", "test-wallet.json");
process.env.ANCHOR_WALLET = process.env.ANCHOR_WALLET ?? defaultWallet;
process.env.SOLANA_WALLET = process.env.SOLANA_WALLET ?? defaultWallet;
process.chdir(workspaceRoot);
// eslint-disable-next-line @typescript-eslint/no-var-requires
const anchor = require("@coral-xyz/anchor");

describe("Solana aggregator demo", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.Vault as Program<Vault>;
  const admin = provider.wallet as anchor.Wallet;

  const ZERO_PUBKEY = new PublicKey("11111111111111111111111111111111");

  let vaultPda: PublicKey;
  let vaultSigner: PublicKey;
  let vaultBump: number;
  let vaultSignerBump: number;

  before(async () => {
    [vaultPda, vaultBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), admin.publicKey.toBuffer()],
      program.programId
    );
    [vaultSigner, vaultSignerBump] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault_signer"), vaultPda.toBuffer()],
      program.programId
    );

    await program.methods
      .initializeVault(new anchor.BN(1_000))
      .accounts({
        vault: vaultPda,
        vaultSigner,
        authority: admin.publicKey,
        systemProgram: SystemProgram.programId,
      })
      .rpc();
  });

  it("runs deposit/harvest/withdraw with referrals", async () => {
    const connection = provider.connection;

    const usdcMint = await createMint(
      connection,
      admin.payer,
      admin.publicKey,
      null,
      6
    );

    const [usdcVaultToken] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault_token"), vaultPda.toBuffer(), usdcMint.toBuffer()],
      program.programId
    );

    const usdcVaultAta = getAssociatedTokenAddressSync(
      usdcMint,
      vaultSigner,
      true
    );

    await program.methods
      .registerToken(10_000)
      .accounts({
        vault: vaultPda,
        authority: admin.publicKey,
        mint: usdcMint,
        vaultSigner,
        vaultToken: usdcVaultToken,
        vaultTokenAccount: usdcVaultAta,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .rpc();

    const alice = Keypair.generate();
    const bob = Keypair.generate();
    await Promise.all(
      [alice, bob].map((kp) =>
        connection.requestAirdrop(kp.publicKey, 2 * LAMPORTS_PER_SOL)
      )
    );

    const aliceUsdc = await getOrCreateAssociatedTokenAccount(
      connection,
      admin.payer,
      usdcMint,
      alice.publicKey
    );
    const bobUsdc = await getOrCreateAssociatedTokenAccount(
      connection,
      admin.payer,
      usdcMint,
      bob.publicKey
    );

    await mintTo(
      connection,
      admin.payer,
      usdcMint,
      aliceUsdc.address,
      admin.payer,
      10_000_000
    );
    await mintTo(
      connection,
      admin.payer,
      usdcMint,
      bobUsdc.address,
      admin.payer,
      10_000_000
    );

    // helper PDA builders
    const positionPda = (owner: PublicKey) =>
      PublicKey.findProgramAddressSync(
        [
          Buffer.from("user_position"),
          usdcVaultToken.toBuffer(),
          owner.toBuffer(),
        ],
        program.programId
      )[0];
    const referralPda = (owner: PublicKey) =>
      PublicKey.findProgramAddressSync(
        [Buffer.from("referral"), vaultPda.toBuffer(), owner.toBuffer()],
        program.programId
      )[0];

    const alicePosition = positionPda(alice.publicKey);
    const aliceReferral = referralPda(alice.publicKey);
    const bobPosition = positionPda(bob.publicKey);
    const bobReferral = referralPda(bob.publicKey);

    // Alice deposit without inviter
    await program.methods
      .deposit(new anchor.BN(5_000_000), ZERO_PUBKEY)
      .accounts({
        vault: vaultPda,
        vaultToken: usdcVaultToken,
        vaultSigner,
        user: alice.publicKey,
        userTokenAccount: aliceUsdc.address,
        vaultTokenAccount: usdcVaultAta,
        userPosition: alicePosition,
        referralRecord: aliceReferral,
        inviterRecord: null,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([alice])
      .rpc();

    // Bob deposit with Alice as inviter
    await program.methods
      .deposit(new anchor.BN(3_000_000), alice.publicKey)
      .accounts({
        vault: vaultPda,
        vaultToken: usdcVaultToken,
        vaultSigner,
        user: bob.publicKey,
        userTokenAccount: bobUsdc.address,
        vaultTokenAccount: usdcVaultAta,
        userPosition: bobPosition,
        referralRecord: bobReferral,
        inviterRecord: aliceReferral,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([bob])
      .rpc();

    const strategyId = 1;
    const [strategyPda] = PublicKey.findProgramAddressSync(
      [Buffer.from("strategy"), usdcVaultToken.toBuffer(), Buffer.from([strategyId])],
      program.programId
    );

    await program.methods
      .registerStrategy(strategyId, 5_000)
      .accounts({
        vault: vaultPda,
        authority: admin.publicKey,
        vaultToken: usdcVaultToken,
        strategy: strategyPda,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

    // Prepare simulated yield
    const adminYieldAta = await getOrCreateAssociatedTokenAccount(
      connection,
      admin.payer,
      usdcMint,
      admin.publicKey
    );
    await mintTo(
      connection,
      admin.payer,
      usdcMint,
      adminYieldAta.address,
      admin.payer,
      1_000_000
    );

    await program.methods
      .harvest(new anchor.BN(1_000_000))
      .accounts({
        vault: vaultPda,
        vaultToken: usdcVaultToken,
        strategy: strategyPda,
        vaultTokenAccount: usdcVaultAta,
        yieldSource: adminYieldAta.address,
        keeper: admin.publicKey,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    await program.methods
      .withdraw(new anchor.BN(1_000_000))
      .accounts({
        vault: vaultPda,
        vaultToken: usdcVaultToken,
        vaultSigner,
        user: bob.publicKey,
        userPosition: bobPosition,
        userTokenAccount: bobUsdc.address,
        vaultTokenAccount: usdcVaultAta,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([bob])
      .rpc();

    const alicePos = await program.account.userPosition.fetch(alicePosition);
    const bobPos = await program.account.userPosition.fetch(bobPosition);
    const vaultTokenState = await program.account.vaultToken.fetch(usdcVaultToken);
    const aliceReferralState = await program.account.referralRecord.fetch(
      aliceReferral
    );

    expect(alicePos.cumulativePoints.gt(new anchor.BN(0))).to.be.true;
    expect(bobPos.shares.toNumber()).to.be.lessThan(3_000_000);
    expect(vaultTokenState.totalUnderlying.toNumber()).to.equal(5_000_000 + 3_000_000 + 1_000_000 - 1_000_000);
    expect(aliceReferralState.pointsFromInvites.toNumber()).to.be.gt(0);
  }).timeout(150_000);

  it("supports multiple asset registrations", async () => {
    const connection = provider.connection;
    const jitoMint = await createMint(
      connection,
      admin.payer,
      admin.publicKey,
      null,
      9
    );
    const [jitoVaultToken] = PublicKey.findProgramAddressSync(
      [Buffer.from("vault_token"), vaultPda.toBuffer(), jitoMint.toBuffer()],
      program.programId
    );
    const jitoVaultAta = getAssociatedTokenAddressSync(
      jitoMint,
      vaultSigner,
      true
    );

    await program.methods
      .registerToken(12_000)
      .accounts({
        vault: vaultPda,
        authority: admin.publicKey,
        mint: jitoMint,
        vaultSigner,
        vaultToken: jitoVaultToken,
        vaultTokenAccount: jitoVaultAta,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .rpc();

    // simple sanity fetch
    const jitoState = await program.account.vaultToken.fetch(jitoVaultToken);
    expect(jitoState.pointsMultiplierBps).to.equal(12_000);
  });
});

