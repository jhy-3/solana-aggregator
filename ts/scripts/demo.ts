#!/usr/bin/env ts-node
/**
 * 独立演示脚本：资产注册和存取款
 * 使用方法：
 *   cd /home/jhy3/develop/defiLab/solana-aggregator
 *   pnpm demo
 */

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
import path from "path";

const workspaceRoot = path.resolve(__dirname, "..", "..");
const anchorToml = path.join(workspaceRoot, "Anchor.toml");
process.env.ANCHOR_CONFIG = process.env.ANCHOR_CONFIG ?? anchorToml;
const defaultWallet = path.join(workspaceRoot, "target", "test-wallet.json");
process.env.ANCHOR_WALLET = process.env.ANCHOR_WALLET ?? defaultWallet;
process.env.SOLANA_WALLET = process.env.SOLANA_WALLET ?? defaultWallet;
// 设置本地测试验证器 URL
process.env.ANCHOR_PROVIDER_URL = process.env.ANCHOR_PROVIDER_URL ?? "http://127.0.0.1:8899";
process.chdir(workspaceRoot);

// eslint-disable-next-line @typescript-eslint/no-var-requires
const anchor = require("@coral-xyz/anchor");
import { Vault } from "../../target/types/vault";

const ZERO_PUBKEY = new PublicKey("11111111111111111111111111111111");

async function main() {
  console.log("🚀 启动 Solana 机枪池演示...\n");

  // 初始化 provider 和 program
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.Vault as Program<Vault>;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const admin = provider.wallet as any;
  const connection = provider.connection;

  console.log("📋 管理员地址:", admin.publicKey.toString());
  console.log("📋 Program ID:", program.programId.toString());

  // 检查并部署程序
  console.log("\n0️⃣ 检查程序部署状态...");
  try {
    const programInfo = await connection.getAccountInfo(program.programId);
    if (!programInfo) {
      console.log("⚠️  程序未部署，正在部署...");
      console.log("   提示：请先运行 'anchor build' 构建程序");
      console.log("   然后运行 'anchor deploy' 部署程序");
      console.log("   或者使用 'anchor test' 自动部署");
      throw new Error(
        "程序未部署。请先运行: anchor build && anchor deploy"
      );
    }
    console.log("✅ 程序已部署");
  } catch (err: any) {
    if (err.message?.includes("程序未部署")) {
      throw err;
    }
    console.log("⚠️  无法检查程序状态，继续尝试...");
  }

  // 计算 Vault PDA
  const [vaultPda, vaultBump] = PublicKey.findProgramAddressSync(
    [Buffer.from("vault"), admin.publicKey.toBuffer()],
    program.programId
  );
  const [vaultSigner, vaultSignerBump] = PublicKey.findProgramAddressSync(
    [Buffer.from("vault_signer"), vaultPda.toBuffer()],
    program.programId
  );

  console.log("\n1️⃣ 初始化 Vault...");
  try {
    await program.methods
      .initializeVault(new anchor.BN(1_000))
      .accounts({
        vault: vaultPda,
        vaultSigner,
        authority: admin.publicKey,
        systemProgram: SystemProgram.programId,
      } as any)
      .rpc();
    console.log("✅ Vault 初始化成功");
    console.log("   Vault PDA:", vaultPda.toString());
  } catch (err: any) {
    if (err.message?.includes("already in use")) {
      console.log("ℹ️  Vault 已存在，跳过初始化");
    } else {
      throw err;
    }
  }

  // 创建 USDC mint
  console.log("\n2️⃣ 创建 USDC 代币...");
  const usdcMint = await createMint(
    connection,
    admin.payer,
    admin.publicKey,
    null,
    6
  );
  console.log("✅ USDC Mint:", usdcMint.toString());

  // 注册 USDC 到 Vault
  const [usdcVaultToken] = PublicKey.findProgramAddressSync(
    [Buffer.from("vault_token"), vaultPda.toBuffer(), usdcMint.toBuffer()],
    program.programId
  );
  const usdcVaultAta = getAssociatedTokenAddressSync(
    usdcMint,
    vaultSigner,
    true
  );

  console.log("\n3️⃣ 注册 USDC 资产到 Vault...");
  try {
    await program.methods
      .registerToken(10_000) // points multiplier: 100%
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
      } as any)
      .rpc();
    console.log("✅ USDC 资产注册成功");
    console.log("   VaultToken PDA:", usdcVaultToken.toString());
  } catch (err: any) {
    if (err.message?.includes("already in use")) {
      console.log("ℹ️  USDC 资产已注册，跳过");
    } else {
      throw err;
    }
  }

  // 创建用户
  console.log("\n4️⃣ 创建测试用户...");
  const alice = Keypair.generate();
  const bob = Keypair.generate();
  console.log("   Alice:", alice.publicKey.toString());
  console.log("   Bob:", bob.publicKey.toString());

  // 给用户空投 SOL
  await Promise.all(
    [alice, bob].map((kp) =>
      connection.requestAirdrop(kp.publicKey, 2 * LAMPORTS_PER_SOL)
    )
  );
  console.log("✅ 用户 SOL 空投完成");

  // 创建用户的 USDC token account 并 mint
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
    10_000_000 // 10 USDC (6 decimals)
  );
  await mintTo(
    connection,
    admin.payer,
    usdcMint,
    bobUsdc.address,
    admin.payer,
    10_000_000
  );
  console.log("✅ 用户 USDC 余额准备完成 (每人 10 USDC)");

  // 计算 PDA
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

  // Alice 存款（无邀请人）
  console.log("\n5️⃣ Alice 存入 5 USDC（无邀请人）...");
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
    } as any)
    .signers([alice])
    .rpc();
  console.log("✅ Alice 存款成功");

  // Bob 存款（Alice 作为邀请人）
  console.log("\n6️⃣ Bob 存入 3 USDC（邀请人：Alice）...");
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
    } as any)
    .signers([bob])
    .rpc();
  console.log("✅ Bob 存款成功，Alice 获得邀请奖励积分");

  // 查询状态
  console.log("\n7️⃣ 查询当前状态...");
  const alicePos = await program.account.userPosition.fetch(alicePosition);
  const bobPos = await program.account.userPosition.fetch(bobPosition);
  const vaultTokenState = await program.account.vaultToken.fetch(usdcVaultToken);
  
  // 读取 referral record（直接从链上读取）
  let aliceInvitePoints = "0";
  try {
    const aliceReferralAccountInfo = await connection.getAccountInfo(aliceReferral);
    if (aliceReferralAccountInfo) {
      // ReferralRecord 布局: discriminator(8) + vault(32) + user(32) + inviter(32) + points_from_invites(16) + bump(1)
      const data = aliceReferralAccountInfo.data;
      // points_from_invites 在偏移 104 (8+32+32+32)，长度 16 字节 (u128)
      const pointsBuffer = data.slice(104, 120);
      const pointsLow = pointsBuffer.readBigUInt64LE(0);
      const pointsHigh = pointsBuffer.readBigUInt64LE(8);
      const points = (pointsHigh << 64n) | pointsLow;
      aliceInvitePoints = points.toString();
    }
  } catch (err) {
    console.log("   ⚠️  无法读取邀请奖励积分");
  }

  console.log("\n📊 账户状态:");
  console.log("   Alice shares:", alicePos.shares.toString());
  console.log("   Alice 累计积分:", alicePos.cumulativePoints.toString());
  console.log("   Bob shares:", bobPos.shares.toString());
  console.log("   Bob 累计积分:", bobPos.cumulativePoints.toString());
  console.log("   Vault 总资产:", vaultTokenState.totalUnderlying.toString());
  console.log("   Vault 总份额:", vaultTokenState.totalShares.toString());
  console.log("   Alice 邀请奖励积分:", aliceInvitePoints);

  // Bob 取款
  console.log("\n8️⃣ Bob 取出 1 USDC...");
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
    } as any)
    .signers([bob])
    .rpc();
  console.log("✅ Bob 取款成功");

  // 最终状态
  console.log("\n9️⃣ 最终状态查询...");
  const finalVaultState = await program.account.vaultToken.fetch(
    usdcVaultToken
  );
  const finalBobPos = await program.account.userPosition.fetch(bobPosition);

  console.log("\n📊 最终状态:");
  console.log("   Vault 总资产:", finalVaultState.totalUnderlying.toString());
  console.log("   Bob 剩余 shares:", finalBobPos.shares.toString());

  console.log("\n✅ 演示完成！");
  console.log("\n💡 提示:");
  console.log("   - 查看完整测试: pnpm test");
  console.log("   - 查看账户数据: anchor account <pubkey>");
  console.log("   - 查看日志: solana logs");
}

main().catch((err) => {
  console.error("❌ 错误:", err);
  process.exit(1);
});

