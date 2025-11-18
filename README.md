# Solana 机枪池 Demo

本仓库提供一个最小可用（MVP）的 Anchor 工程骨架，覆盖：

- `programs/vault`: 单一金库 Program，包含 `initialize_vault / deposit / withdraw / harvest / update_params` 指令与事件。
- `ts/tests/vault.ts`: 使用 `@coral-xyz/anchor` 的脚本，后续可扩展成前端/keeper 测试。
- `Anchor.toml` + `Cargo.toml`: Anchor/Cargo workspace 配置。

## 快速开始

1. **安装依赖**

   ```bash
   pnpm install
   pnpm install --filter ts-tests
   ```

   若本地缺少 Anchor CLI，可参考官方仓库 `https://github.com/coral-xyz/anchor` 手动安装（需要 `libudev`）。项目采用 **Anchor 0.32.1**，推荐使用 `avm` 切换：

   ```bash
   cargo install --git https://github.com/coral-xyz/anchor avm --locked
   avm install 0.32.1 && avm use 0.32.1
   ```

2. **生成 IDL**

   ```bash
   anchor build
   pnpm run build:idl
   ```

3. **运行示例脚本**

   ```bash
   anchor test    # 启动本地 validator + 运行 ts/tests/vault.ts
   # 或单测 TypeScript 流程
   pnpm test
   ```

`anchor test` 会自动：
- 启动本地 test validator
- 构建 `programs/vault`
- 运行 `ts/tests/vault.ts`，演示多资产注册、存取款、模拟收益、积分与邀请积分

## Demo 要点

- **Vault Program (`programs/vault`)**
  - 指令：`initialize_vault / update_vault_params / register_token / register_strategy / deposit / withdraw / harvest`
  - 份额制 accounting，Vault PDA 托管资产；`harvest` 由 keeper 把策略收益 C P I 回灌
  - 事件：`DepositEvent / WithdrawEvent / HarvestEvent / StrategyRegistered` 方便后续 indexer/UI

- **积分与邀请**
  - `UserPosition`：记录 shares、时间戳、累计积分；积分 = amount × time × base_rate × multiplier
  - `ReferralRecord`：绑定 inviter（只能设置一次），`deposit` 自动给 inviter 累计 bonus（默认 5%）

- **多资产机枪池骨架**
  - 单 Vault，多 `VaultToken`（USDC/jitoSOL/WBTC/JUP...）；每个 token 可单独设置 multiplier
  - `Strategy` PDA 用来登记底层策略/权重；Demo 用 simulated yield，后续可接 Jito/Kamino 等
  - `ts/tests/vault.ts` 创建 2 种资产，跑通 “存 → 邀请 → harvest → 取” 闭环

- **调试方法**
  - `anchor account <pubkey>` 查看账户数据
  - `solana logs` / `anchor test -- --grep ...` 查看事件

## 后续路线

- 扩展 `harvest`：用 CPI 接入真实策略（Jito、Kamino、MarginFi 等）。
- 集成 Keeper：在 `ts/` 目录新增 cron 脚本，定期触发 `harvest`。
- Indexer & UI：监听事件写入数据库/Helius，再在 Next.js 前端展示 TVL/APY。
- 治理与多签：结合 Squads / Realms 管理 `authority`。

欢迎根据 `guid.md` 与需求 PDF 继续拆解子任务。***
