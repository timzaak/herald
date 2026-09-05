# Passkey 认证产品需求文档 (PRD)

**创建时间**: 2026-07-07
**优先级**: P0

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 故事引用

- `[US-PK-001]` Realm 管理员启用/禁用 Passkey 功能，优先级 P0，来源 `docs/user-stories/auth/passkey.md`
  - 角色：Realm Admin
  - 摘要：管理员为本 Realm 启用或禁用 Passkey 认证功能
- `[US-PK-002]` Realm 管理员强制启用 Passkey，优先级 P0，来源 `docs/user-stories/auth/passkey.md`
  - 角色：Realm Admin
  - 摘要：管理员设置本 Realm 强制使用 Passkey
- `[US-PK-003]` Realm 管理员配置 Passkey 安全策略，优先级 P1，来源 `docs/user-stories/auth/passkey.md`
  - 角色：Realm Admin
  - 摘要：管理员配置用户验证要求、跨平台 authenticator 策略等
- `[US-PK-004]` 用户注册 Passkey，优先级 P0，来源 `docs/user-stories/auth/passkey.md`
  - 角色：Regular User
  - 摘要：用户在安全设置页注册 Passkey，支持多设备
- `[US-PK-005]` 用户使用 Passkey 直接登录，优先级 P0，来源 `docs/user-stories/auth/passkey.md`
  - 角色：Regular User
  - 摘要：用户通过 usernameless / conditional UI 使用 Passkey 直接登录
- `[US-PK-006]` 用户在密码登录后使用 Passkey 作为第二因素，优先级 P0，来源 `docs/user-stories/auth/passkey.md`
  - 角色：Regular User
  - 摘要：用户在密码验证通过后使用 Passkey 完成二次验证
- `[US-PK-007]` 用户查看和重命名已注册 Passkey，优先级 P0，来源 `docs/user-stories/auth/passkey.md`
  - 角色：Regular User
  - 摘要：用户查看、重命名已注册的 Passkey 设备
- `[US-PK-008]` 用户在无法使用 Passkey 时回退到密码/TOTP，优先级 P0，来源 `docs/user-stories/auth/passkey.md`
  - 角色：Regular User
  - 摘要：用户在浏览器不支持、验证失败或设备不可用时回退到密码/TOTP
- `[US-PK-009]` 用户删除 Passkey，优先级 P0，来源 `docs/user-stories/auth/passkey.md`
  - 角色：Regular User
  - 摘要：用户删除不再使用的 Passkey 设备
- `[US-PK-010]` Realm 管理员查看 Passkey 使用情况统计，优先级 P2，来源 `docs/user-stories/auth/passkey.md`
  - 角色：Realm Admin
  - 摘要：管理员查看本 Realm Passkey 启用率和登录统计

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 8 | 管理员开关/强制、用户注册、直接登录、第二因素、设备管理、回退、删除 |
| P1 | 1 | 管理员配置安全策略 |
| P2 | 1 | 管理员查看使用统计 |

---

## 2. 范围界定

### 2.1 包含功能

- Realm 级别 Passkey 开关（管理员可启用/禁用）
- Realm 级别强制 Passkey 模式（引导未注册用户注册，但仍保留回退）
- Realm 级别 Passkey 安全策略配置（用户验证要求、跨平台 authenticator 策略，P1）
- 用户注册 Passkey（支持多设备、设备命名）
- Passkey 作为第一因素直接登录（支持 usernameless / conditional UI）
- Passkey 作为第二因素（密码登录后使用 Passkey 验证）
- 用户查看、重命名、删除已注册 Passkey
- 删除最后一个 Passkey 时的明确风险提示
- 当 Passkey 不可用时回退到密码/TOTP 登录
- 浏览器不支持 WebAuthn 时的降级显示
- Realm 管理员查看 Passkey 启用率与登录统计（P2）
- 审计日志记录关键 Passkey 事件（注册、删除、登录、策略变更）

### 2.2 不包含功能 (Out of Scope)

- 跨 Realm 共享 Passkey credential
- 企业级 attestation 验证与 MDS（Metadata Service）校验
- 管理员远程强制删除或锁定单个用户 Passkey（可通过用户管理重置账户的认证方式，但不单独管理 Passkey）
- 在未注册任何 Passkey 的设备上实现无用户名跨设备登录（依赖平台生态，应用层无法控制）
- 完全禁用密码/TOTP、仅允许 Passkey 的"纯无密码"模式（必须保留回退）
- Passkey 专属恢复码机制（沿用 TOTP 备份恢复码作为通用回退）
- 短信/邮箱验证码作为 Passkey 失败后的专属回退通道
- 用户级 Passkey 自动过期策略

### 2.3 依赖项

- 用户认证系统 — 提供登录、会话管理和密码验证
- Realm 配置系统 — 存储 Realm 级别 Passkey 开关与策略
- Session 管理 — Passkey 验证通过后创建 Session
- TOTP 系统 — 作为 Passkey 不可用时的一种回退认证方式
- WebAuthn RP 库 — 后端 challenge 生成、attestation/assertion 验证，采用 [passkey-auth 0.1](https://crates.io/crates/passkey-auth)（纯 Rust，RustCrypto end-to-end，不依赖 openssl）
- 浏览器 Web Authentication API — 前端创建/获取 credential
- HTTPS 生产环境 — WebAuthn 规范强制要求
- 多 RP 解析模型 — 默认使用环境变量 `RP_ID`/`RP_ORIGIN`；Client App 配置了 `passkey_rp` 时以该 Client App 的 origin 为 RP；已生效的自定义域名优先于环境变量（三级匹配 `resolve_passkey_rp`）。credential 唯一性按 `(realm, user, rp_id, credential_id)` 隔离，同一用户在不同 RP 下持有各自独立的 passkey，设备列表按当前请求解析出的 RP 过滤

---

## 3. 需求概述

### 3.1 功能描述

Passkey 基于 WebAuthn / FIDO2 标准，提供无密码、防钓鱼的公钥加密认证能力。Herald 系统在 Realm 级别和用户级别同时支持 Passkey：

- **Realm 级别**：管理员决定是否启用 Passkey、是否强制用户使用，以及配置安全策略（如用户验证要求）。
- **用户级别**：用户可以在 Realm 允许时注册一个或多个 Passkey，并使用 Passkey 直接登录或作为密码后的第二因素。

Passkey 同时支持两种认证场景：
1. **第一因素登录**：用户访问登录页后，系统通过 conditional UI 提示可用 Passkey，用户选择并完成验证后直接登录，无需输入密码。
2. **第二因素验证**：用户先输入邮箱和密码，验证通过后再使用 Passkey 完成二次验证，与现有 TOTP 二次认证模式并列。

系统必须始终保留密码（以及已启用的 TOTP）作为回退方式，防止因设备丢失、浏览器不兼容或平台生态限制导致账户锁定。

### 3.2 关键特性

- **双模式认证**：同一 Passkey credential 既可作为第一因素登录，也可在密码登录后作为第二因素。
- **Usernameless / Conditional UI**：登录页支持自动填充可用 Passkey，已注册用户无需手动输入邮箱即可选择凭证。
- **多设备管理**：用户可注册多个 Passkey，并为每个凭证设置可识别名称、查看最近使用时间、删除指定设备。
- **Realm 级策略**：沿用 TOTP 的 `enabled` / `force_enabled` 模式，并扩展可选的安全策略配置。
- **安全默认**：challenge 一次性且限时；验证 origin 与 RP_ID；校验签名计数器防止 credential 克隆；私钥始终留在用户设备。
- **平滑降级**：Realm 禁用 Passkey 后，新用户无法注册，已注册用户仍可回退到密码/TOTP；浏览器不支持时自动隐藏 Passkey 入口。

---

## 4. 业务规则与状态

### 4.1 业务规则

- **Realm 开关规则**：Realm 管理员可启用/禁用 Passkey 功能。禁用后新用户无法注册 Passkey，已注册用户仍可继续使用已注册凭证登录或回退到密码/TOTP。
- **强制 Passkey 模式规则**：Realm 管理员可开启强制模式。强制模式下，未注册 Passkey 的用户下次登录时被引导注册，但系统必须保留密码/TOTP 回退入口，防止用户因设备或浏览器限制被锁定。该强制引导为**前端读取 realm config 后的 UI 行为**，后端不阻断登录（登录成功响应可携带引导信号供前端消费）。
- **多设备规则**：一个用户可以拥有多个 Passkey credential。同一 credential ID 在一个 realm 内必须唯一。
- **设备命名规则**：注册成功后系统显示默认设备名（如"iCloud Keychain"、"YubiKey"或浏览器提示的 authenticator 名称），用户可在管理页修改。
- **删除规则**：删除单个 Passkey 后立即失效；删除最后一个 Passkey 前，系统必须明确提示用户将只能使用密码/TOTP 登录。
- **回退规则**：Passkey 验证失败、浏览器不支持、用户取消或无可用的 Passkey 时，必须提供切换到密码登录的入口；若用户已启用 TOTP，密码登录后按现有 TOTP 流程继续。
- **Challenge 规则**：注册和认证流程中的 challenge 必须一次性使用且设置有效期（建议 5 分钟），验证成功后立即失效。
- **速率限制规则**：Passkey 注册和认证接口应用与现有登录/认证接口一致的速率限制策略。
- **用户验证策略规则**：Realm 可配置用户验证（User Verification）要求为 `preferred` 或 `required`；注册和认证流程按当前策略执行。
- **跨平台 Authenticator 规则**：Realm 可配置是否允许跨平台 authenticator（如 YubiKey、手机作为漫游 authenticator）；默认允许以兼容常见 passkey 同步生态。
- **安全存储规则**：服务器仅存储 credential ID、公钥（COSE）、签名计数器、transports、aaguid、backup eligibility/state、设备昵称和元数据；私钥不得离开用户设备，也不得在服务端持久化。
  > **已知限制**：passkey-auth 0.1 不暴露 BE/BS（backup eligibility/state）flags，因此这两个字段当前恒为 `false`，sync passkey 同步状态展示失真。后续需升级库或调整字段语义。
- **审计规则**：关键事件（注册成功、删除 credential、管理员变更 Passkey 策略、强制模式变更、Passkey 登录成功/失败）应记录审计日志。

### 4.2 关键状态与异常

- **Realm Passkey 未启用**：用户看不到 Passkey 注册入口和登录选项。
- **Realm Passkey 已启用，用户未注册**：登录页显示 Passkey 选项，用户选择后系统提示未找到可用 Passkey，并引导注册或切换密码登录。
- **Realm Passkey 已启用，用户已注册**：登录页支持 conditional UI，用户可直接选择 Passkey 登录。
- **Realm 强制 Passkey 模式，用户未注册**：登录成功后或登录流程中提示用户注册 Passkey，但允许跳过并使用密码/TOTP。
- **浏览器不支持 WebAuthn**：自动隐藏 Passkey 入口，仅显示密码/TOTP 登录。
- **Passkey 验证失败**：统一提示"Passkey 验证失败"，不暴露具体原因；提供重试和回退入口。
- **无可用的 Passkey**：系统提示"未找到可用的 Passkey"，并允许切换到密码登录。
- **Credential 被删除**：该 credential 不能再用于登录；若为用户最后一个 credential，下次登录不再显示 conditional UI 提示。

---

## 5. 功能需求

### 5.1 核心需求

- **Realm 级别 Passkey 配置**：管理员在 Settings -> Security 页面控制 Passkey 开关、强制模式和基础安全策略。启用率与登录统计接口（US-PK-010）为 P2，本期未实现。
- **用户注册 Passkey**：已登录用户在个人资料 -> Security 页面发起注册，系统与浏览器交互创建 credential，成功后显示设备名称并允许用户修改。
- **Passkey 第一因素登录**：登录页支持 conditional UI，已注册用户在聚焦用户名输入框时自动收到 Passkey 提示；也可主动点击"Use Passkey"按钮。
- **Passkey 第二因素登录**：用户在输入邮箱和密码后，若已启用 Passkey 作为第二因素，则进入 Passkey 验证步骤；验证通过后创建 Session。
- **用户设备管理**：用户在 Security 页面查看所有已注册 Passkey 列表，包括设备名、注册时间、最近使用时间、同步状态；支持重命名和删除。
- **回退与降级**：登录流程始终提供"Use password instead"入口；浏览器不支持时隐藏 Passkey 选项；强制模式下仍保留回退。
- **审计与可观测性**：记录 Passkey 相关关键事件到审计日志，管理员可查看启用率与登录方式分布统计（P2）。

### 5.2 验收目标

- 用户可在支持的浏览器中完成 Passkey 注册、直接登录、作为第二因素登录的全流程。
- 同一用户可在多个设备上注册 Passkey，并能在 Security 页面管理这些设备。
- 当 Passkey 不可用时，用户可顺利回退到密码/TOTP 登录。
- Realm 禁用 Passkey 后，新用户无法注册，已启用用户不受影响并可回退。
- 强制模式下，未注册 Passkey 的用户被引导注册，但不会因无 Passkey 而被锁定。
- 浏览器不支持 WebAuthn 时，登录页不显示 Passkey 选项且密码登录正常可用。
- Passkey 验证失败时，系统提示统一且不暴露具体原因，同时提供重试和回退入口。

---

## 6. API 相关约束

**适用性**: 适用

- **接口能力范围**：Passkey 注册 challenge 生成与完成、Passkey 认证 challenge 生成与完成、用户已注册 Passkey 列表查询/重命名/删除、Realm 级别 Passkey 开关与策略配置、Passkey 启用率统计查询。
- **访问控制**：Realm Admin 可操作 Realm 级别 Passkey 配置和统计；Regular User 仅可操作自身 Passkey 设置；Passkey 注册和管理操作需在已认证 Session 内进行；Passkey 登录为未认证接口，需应用速率限制。
- **数据边界**：Passkey credential 数据按 realm 隔离；credential ID 在 realm 内唯一；响应中不返回公钥等敏感元数据。
- **安全约束**：challenge 一次性且限时；验证 origin 与 RP_ID 必须匹配当前部署配置；签名计数器递增校验防止克隆；验证失败不暴露具体原因；注册和认证接口应用速率限制。
- **兼容性约束**：接口设计需支持 usernameless（discoverable credential）和 non-discoverable credential 两种场景；前端需处理不同浏览器对 transports、user verification 的差异。

---

## 7. 前端/交互约束

**适用性**: 适用

- **页面入口**：
  - 管理员：Settings -> Security 页面管理 Passkey 开关、强制模式和安全策略。
  - 用户：个人资料 -> Security 页面管理 Passkey 设备（注册、查看、重命名、删除）。
  - 登录页：提供 Passkey 登录入口，并支持 conditional UI 自动填充。
- **关键用户路径**：
  - 注册 Passkey：进入 Security 页面 -> 点击"Add Passkey" -> 浏览器弹窗完成验证 -> 显示设备名 -> 可修改名称。
  - 直接登录：访问登录页 -> 系统自动提示可用 Passkey（conditional UI）或点击"Use Passkey" -> 完成验证 -> 登录成功。
  - 第二因素登录：输入邮箱密码 -> 验证通过 -> 显示 Passkey 验证提示 -> 完成验证 -> 登录成功。
  - 管理设备：Security 页面显示列表 -> 点击编辑名称或删除 -> 删除最后一个时弹出风险提示。
- **状态反馈**：
  - 注册成功：显示设备名和"注册成功"提示。
  - 验证失败：统一提示"Passkey 验证失败"，提供重试和"使用密码登录"选项。
  - 浏览器不支持：隐藏 Passkey 入口，提示"当前浏览器不支持 Passkey"。
  - 无可用的 Passkey：提示"未找到可用的 Passkey"，引导切换密码。
  - 强制模式未注册：登录后提示引导注册 Passkey，但保留跳过入口。
- **权限可见性**：Realm 未启用 Passkey 时，用户看不到 Passkey 注册入口和登录选项；强制模式下禁用/删除最后一个 Passkey 需明确提示风险。
- **异常提示**：challenge 超时需重新发起；用户取消浏览器弹窗时不报错，保持页面可继续操作；网络异常时给出通用错误提示。

---

## 8. 已确认决策

- Passkey 同时支持第一因素登录和第二因素验证，两种模式均为 P0。
- 第一因素登录支持 usernameless / discoverable credential 的 conditional UI 自动填充体验。
- 系统保留密码和/或 TOTP 作为回退认证方式，不实现"纯无密码"模式（强制模式下也必须保留回退）。
- 一个用户可以拥有多个 Passkey credential，并可在 Security 页面管理。
- Realm 级别沿用 TOTP 的 `enabled` / `force_enabled` 开关模式，并扩展可选安全策略配置。
- 后端采用经过安全审计的 WebAuthn RP 库，不自行实现密码学验证。
- 生产环境必须 HTTPS，RP_ID 与 RP_ORIGIN 需按部署环境配置，不硬编码（作为未配置 Client App origin / 自定义域名时的默认 RP）。
- 服务器不存储私钥；仅存储 credential ID、公钥、计数器、transports、backup 状态、设备昵称等元数据。

---

## 9. 参考资料

- 用户故事：`docs/user-stories/auth/passkey.md`
- 相关 PRD：`docs/prd/auth/totp.md`（TOTP 作为 Passkey 不可用时的回退认证方式）
- 角色定义：`docs/user-stories/_roles.md`
- PRD 索引：`docs/prd/index.md`
- WebAuthn / FIDO2 规范
