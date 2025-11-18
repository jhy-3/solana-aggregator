# 快速启动指南

## 运行资产注册和存取款演示

### 步骤 1: 构建程序

首先需要构建程序：

```bash
cd /home/jhy3/develop/defiLab/solana-aggregator
anchor build
```

### 步骤 2: 启动本地 Solana 测试验证器并部署程序

在一个终端窗口中运行：

```bash
solana-test-validator
```

保持这个终端窗口运行。

在另一个终端窗口中部署程序：

```bash
cd /home/jhy3/develop/defiLab/solana-aggregator
anchor deploy
```

### 步骤 3: 运行演示脚本

在同一个终端窗口中运行：

```bash
pnpm demo
```

**或者，使用便捷命令一键完成构建、部署和演示：**

```bash
# 确保 solana-test-validator 在运行，然后：
pnpm demo:full
```

**或者，使用 `anchor test` 一键完成构建、部署和测试：**

```bash
anchor test
```

### 演示流程

演示脚本会自动执行以下操作：

1. **初始化 Vault** - 创建主金库
2. **创建 USDC 代币** - 创建测试用的 USDC mint
3. **注册 USDC 资产** - 将 USDC 注册到 Vault
4. **创建测试用户** - 创建 Alice 和 Bob 两个用户
5. **Alice 存款** - Alice 存入 5 USDC（无邀请人）
6. **Bob 存款** - Bob 存入 3 USDC（Alice 作为邀请人，获得邀请奖励）
7. **查询状态** - 显示所有账户的 shares、积分等信息
8. **Bob 取款** - Bob 取出 1 USDC
9. **最终状态** - 显示最终的金库和用户状态

### 输出示例

```
🚀 启动 Solana 机枪池演示...

📋 管理员地址: 8XNwnTXDij5mvse39KiDm3nXaP9BWu2xnoLaktRjGLxa
📋 Program ID: EcYQSDeJyV4VpFUbBQbVFzdd1Ta4ZFvQY7ViCUGWd1EY

1️⃣ 初始化 Vault...
✅ Vault 初始化成功
   Vault PDA: ...

2️⃣ 创建 USDC 代币...
✅ USDC Mint: ...

3️⃣ 注册 USDC 资产到 Vault...
✅ USDC 资产注册成功
   VaultToken PDA: ...

...

📊 账户状态:
   Alice shares: 5000000
   Alice 累计积分: ...
   Bob shares: 3000000
   ...

✅ 演示完成！
```

### 其他运行方式

#### 运行完整测试套件

```bash
anchor test
```

这会运行所有测试，包括：
- 资产注册和存取款
- 邀请奖励机制
- Harvest 收益复投
- 多资产支持

#### 查看账户数据

```bash
# 查看 Vault 账户
anchor account <vault_pda>

# 查看用户持仓
anchor account <user_position_pda>
```

#### 查看交易日志

```bash
# 在运行 solana-test-validator 的终端中查看日志
# 或者使用
solana logs
```

### 常见问题

**Q: 提示 "Program is not deployed"**
A: 确保先运行了 `anchor build`，并且 `solana-test-validator` 正在运行。

**Q: 提示 "Insufficient funds"**
A: 确保测试验证器正在运行，脚本会自动请求空投。

**Q: 想使用自己的钱包**
A: 设置环境变量：
```bash
export ANCHOR_WALLET=/path/to/your/wallet.json
export SOLANA_WALLET=/path/to/your/wallet.json
pnpm demo
```

### 下一步

- 查看 `scripts/demo.ts` 了解如何自定义演示
- 查看 `ts/tests/vault.ts` 了解完整的测试用例
- 查看 `programs/vault/src/lib.rs` 了解程序实现细节

