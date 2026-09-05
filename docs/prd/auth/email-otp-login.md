# 邮箱验证码登录 产品需求文档 (PRD)

**创建时间**: 2026-07-17
**优先级**: P1
**所属域**: auth

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/auth/email-otp-login.md`。

### 1.1 故事引用

- `[US-EO-001]` 用户用邮箱验证码登录已有账号，优先级 P0，来源 `docs/user-stories/auth/email-otp-login.md`
  - 角色：Regular User
  - 摘要：用户在登录入口选择邮箱验证码登录，输入邮箱并验证码即可登录，免记忆密码
- `[US-EO-002]` 未注册邮箱验证成功后自动注册，优先级 P0，来源 `docs/user-stories/auth/email-otp-login.md`
  - 角色：Regular User
  - 摘要：未注册邮箱经用户同意并验证成功后，一次操作同时完成登录与账户创建
- `[US-EO-003]` Realm 管理员配置邮箱验证码登录与自动注册，优先级 P1，来源 `docs/user-stories/auth/email-otp-login.md`
  - 角色：Realm Admin
  - 摘要：管理员为本 Realm 启用/关闭邮箱验证码登录，并单独控制自动注册开关

表达既有登录、注册、认证器共存语义，不复制验收标准的既有故事：

- `docs/user-stories/core/regular-user.md`（US-RU-001 注册、US-RU-002 登录）
- `docs/user-stories/auth/passkey.md`（Passkey 作为增强/未来主入口）
- `docs/user-stories/core/legal-consent-account-deletion.md`（注册即同意 / 登录即同意模型）

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 2 | US-EO-001 验证码登录、US-EO-002 自动注册 |
| P1 | 1 | US-EO-003 Realm 管理员配置 |

---

## 2. 范围界定

### 2.1 包含功能

- **邮箱验证码登录已有账号**：用户用邮箱接收一次性验证码完成登录，作为密码登录的低摩擦替代入口。
- **未注册邮箱自动注册**：未注册邮箱在用户完成同意表达并验证成功后，自动创建并激活账户；自动注册视为注册路径，不要求用户再走单独的邮箱验证。
- **邮箱所有权验证**：验证码验证即完成邮箱所有权验证；邮箱仍是账户必填身份与恢复渠道。
- **客户端会话承接**：登录成功后承接自建用户 UI（custom-user-ui）已确认的 Bearer access/refresh token 方向，不退回 cookie-only 假设。
- **per-Realm 启停**：Realm 管理员可启用/关闭邮箱验证码登录，并单独控制自动注册开关。
- **首期投放范围**：产品首期仅在 ai-agent-app 对应 Realm 启用；后端不硬编码 Realm 名称，而以每个 Realm 的 `email_otp` 开关承载投放，其他 Realm 默认关闭。
- **与其他登录方式共存**：保留密码登录入口；Passkey 作为用户后续可绑定的增强/未来主入口；OTP 不替代现有 TOTP 二因素或高危操作重新认证。

### 2.2 不包含功能 (Out of Scope)

- **无邮箱账户或纯 Passkey 注册**：邮箱仍是必填身份；不引入无邮箱账户模型。
- **取消密码、找回密码或现有二因素能力**：这些能力保持现状。
- **重新定义 Herald 的通用 passwordless 平台战略**：本功能是 ai-agent-app 的登录摩擦解决方案，不是平台完整性建设。
- **默认向所有 Realm 开启自动注册**：首期不默认开放，推广由数据驱动。
- **客户端会话机制设计**：Bearer access/refresh token 的传输、续期、复用检测、吊销等由自建用户 UI PRD 承载，本 PRD 只声明依赖。
- **邮件投递基础设施改造**：复用现有邮件通道；投递表现的观测和改进作为首期验证项，不在本 PRD 设计新基础设施。
- **Kill Criteria 的具体数值阈值**：观察周期和最低样本量在上线前根据预期用户规模确定，不写入本 PRD 正文。

### 2.3 依赖项

- **自建用户 UI 的 Bearer 登录能力**（强依赖，发布顺序约束）：OTP 登录成功后签发的会话必须按 Bearer access/refresh token 方向建立。该能力由自建用户 UI PRD 承载（见 [docs/prd/integration/custom-user-ui.md](../integration/custom-user-ui.md)）；自建用户 UI 的 Bearer 登录能力须先于或同期交付，本 PRD 不重复设计会话机制。
- **Client App 级 Turnstile 防护（防护层级下放依赖，见 D-PROTECT-01）**：人机验证（Turnstile）配置在 Client App 级别——每个 Client App 配置自己的 Turnstile site_key/secret_key 与启用开关，`realm_config` 不承载 Turnstile。OTP 登录/自动注册的人机验证按**当前请求绑定的 Client App** 的 Turnstile 配置执行。维持 IP/identifier 限流，不新增 client 维度限流。Client App 级 Turnstile 配置能力本身不在本 PRD 定义，由 Client App PRD 承载（见 [docs/prd/integration/client-app.md](../integration/client-app.md)）；本 PRD 只声明依赖。
- **现有协议同意模型**：复用合规适配 PRD 的"登录即同意"语义承载自动注册的有效同意（见 [docs/prd/core/legal-consent-account-deletion.md](../core/legal-consent-account-deletion.md)）。
- **现有用户与 Realm 基础设施**：账户实体、状态枚举、Realm 隔离与 per-Realm 设置承载开关（见 [docs/prd/core/users.md](../core/users.md)、[docs/prd/core/realm-settings.md](../core/realm-settings.md)）。
- **现有邮件通道**：用于发送验证码邮件。

---

## 3. 需求概述

### 3.1 功能描述

ai-agent-app 手机用户首次进入和再次登录存在摩擦：当前只能使用邮箱密码，忘记密码时要走重置流程；Passkey 当前尚未普及，不能假设为主流入口。用户需要一种无需记忆密码、又普遍熟悉的注册与登录方式。

邮箱验证码登录允许用户输入邮箱、接收一次性验证码、验证成功即登录；对未注册邮箱，在用户完成同意表达后自动创建并激活账户，使一次操作同时完成登录与注册。这直接降低首次进入和再次登录的摩擦，验证"邮箱 OTP 登录并自动注册"对 ai-agent-app 手机用户的价值。

### 3.2 关键特性

- **低摩擦登录**：用邮箱验证码替代密码，免记忆、免输入密码。
- **自动注册**：未注册邮箱验证成功后自动创建激活账户，登录与注册一次完成。
- **同意即注册**：自动注册承接"登录即同意"语义，用户表达同意后才创建账户，满足注册政策与协议同意要求。
- **首期限定验证**：只在 ai-agent-app 对应 Realm 开放，用选择率、完成率和放弃率等真实数据决定是否推广。
- **会话方向承接**：不退回 cookie-only，承接 Bearer access/refresh token 方向。

---

## 4. 业务规则与状态

### 4.1 业务规则

- **首期范围边界**：运营仅为 ai-agent-app 对应 Realm 打开配置；代码按 per-Realm 配置执行，不识别或硬编码 ai-agent-app。其他 Realm 默认关闭，推广由首期观测数据决定。
- **邮箱必填**：邮箱仍是账户必填身份与恢复渠道；验证码完成邮箱所有权验证；不引入无邮箱账户。
- **自动注册即注册路径**：自动注册被视为注册路径而非普通登录异常分支；必须满足当前 Realm 生效协议的同意表达（"登录即同意"语义）后才创建账户。
- **有效同意构成**：用户输入邮箱 → 表达同意（勾选/点击"同意协议并继续"）→ 发送验证码 → 验证成功即视作对当前生效用户协议/隐私政策版本的有效同意并被记录；同意记录与具体协议版本绑定，复用既有审计。
- **登录与自动注册的分流**：邮箱已存在账户 → 验证成功即登录；邮箱不存在 → 经同意并验证成功后创建激活账户；两种结果都按客户端会话方向建立会话。
- **凭证定位**：邮箱验证码是便利入口，不作为与 Passkey 同等级的强认证；不得绕过现有 TOTP 二因素、高危操作重新认证或 Realm 注册政策。
- **与其他认证方式共存**：保留密码登录入口；Passkey 作为用户后续可绑定的增强/未来主入口；用户可随后设置密码或绑定 Passkey/TOTP，账户身份不变。
- **per-Realm 开关**：Realm 管理员可启用/关闭邮箱验证码登录，并单独控制自动注册；关闭后平滑降级，已注册用户仍可用密码/TOTP/Passkey 登录。
- **客户端会话方向**：登录成功签发的会话承接自建用户 UI 的 Bearer access/refresh token 方向；不退回 cookie-only。
- **防滥用**：人机验证（Turnstile）按**当前请求绑定的 Client App** 的 Turnstile 配置执行（Client App 级，见 D-PROTECT-01）；维持 IP/identifier 限流；验证码对发送频率、尝试次数、有效期和一次性消费设定上限；首期限定目标 Realm 以控制批量发送与批量注册成本。
- **防枚举**：验证码发送对存在但非激活的账户返回与正常发送一致的反馈，不暴露账户是否存在。
- **注册政策优先**：自动注册不得绕过 Realm 注册政策；当 Realm 未开启自动注册（或不在首期开放范围）时，未注册邮箱只能得到未注册提示或引导到显式注册入口，不创建账户。

### 4.2 关键状态与异常

- **验证码错误/过期**：API 返回统一错误语义；前端可本地化显示“验证码错误或已失效”并提供重发入口。达到连续错误上限后该次验证码作废。
- **发送频率受限**：API 返回 429，不发送新验证码；用户可见文案由前端本地化承载。
- **验证码被重复使用**：一次性消费，成功登录后再次提交同一验证码被拒绝并提示已失效。
- **账号被禁用**：即使验证码正确也拒绝登录，提示账号已被禁用。
- **自动注册缺少同意表达**：不发送验证码或拒绝继续，不创建账户。
- **Realm 不允许自动注册**：未注册邮箱不创建账户，提示未注册或引导显式注册。
- **邮件延迟 / 投递失败**：不视为登录成功；保留密码入口作为回退；首期观测投递表现，若频繁阻塞则不以 OTP 作为主要入口（见 Kill Criteria）。
- **Realm 未启用邮箱验证码登录**：登录页不展示该入口；已注册用户走既有登录方式。
- **跨 Realm 访问配置**：Realm 管理员只能配置本 Realm，跨 Realm 访问被拒绝。

---

## 5. 功能需求

### 5.1 核心需求

- **FR-1（验证码登录已有账号）**：用户在登录入口选择"邮箱验证码登录"，输入邮箱并完成人机验证（若启用），在限定时间内收到并输入正确验证码后登录成功；会话按客户端会话方向（Bearer token）建立。
- **FR-2（未注册邮箱自动注册）**：未注册邮箱在用户完成当前 Realm 生效协议的同意表达并验证成功后，自动创建处于已验证/已激活状态的账户；不要求用户再走单独的邮箱验证；会话按客户端会话方向建立。
- **FR-3（同意闸门）**：自动注册路径在发送验证码前要求用户表达对当前生效协议版本的同意；未表达同意不发送验证码、不创建账户；同意记录与具体协议版本绑定并可审计。
- **FR-4（验证码生命周期）**：验证码有有效期、一次性消费、连续错误上限和发送频率限制。
- **FR-5（per-Realm 启停）**：Realm 管理员可启用/关闭邮箱验证码登录，并单独控制自动注册开关；关闭后平滑降级。
- **FR-6（与其他登录方式共存）**：保留密码登录入口；Passkey 作为后续可绑定入口；OTP 不替代二因素或高危操作重新认证。
- **FR-7（客户端会话承接）**：登录成功签发的会话承接自建用户 UI 的 Bearer access/refresh token 方向；不退回 cookie-only；依赖自建用户 UI Bearer 登录能力先于或同期交付。
- **FR-8（首期范围限定）**：首期只对 ai-agent-app 对应 Realm 开放；不默认向所有 Realm 开启自动注册。
- **FR-9（公开状态）**：提供无需登录的 Realm 邮箱 OTP 启用状态查询，仅返回 `enabled`，供登录页决定是否展示入口。

### 5.2 验收目标

- 已注册且已激活用户用邮箱验证码一次操作完成登录，无需输入密码。
- 未注册邮箱经用户表达同意并验证成功后自动创建激活账户，登录与注册一次完成；账户身份与后续设置的密码/Passkey/TOTP 共存一致。
- 自动注册路径缺少同意表达时不发送验证码、不创建账户。
- Realm 未开启自动注册时，未注册邮箱不创建账户，得到未注册提示或显式注册引导。
- 验证码错误、过期、重复使用和发送频率受限均按 §4.2 反馈，无法通过穷举或重放绕过。
- 被禁用账号即使验证码正确也被拒绝登录。
- 登录成功签发的会话按 Bearer access/refresh token 方向建立，不依赖浏览器 cookie。
- Realm 管理员可启用/关闭邮箱验证码登录，并单独控制自动注册；关闭后已注册用户仍可用密码/TOTP/Passkey 登录。
- 首期范围之外的 Realm 不展示该入口或不允许自动注册。
- 跨 Realm 配置访问被拒绝。

---

## 6. API 相关约束

**适用性**: 适用

- **能力边界**：新增邮箱验证码发送与验证（登录/自动注册）能力，作为未认证身份端点公开开放；不新增管理员以外的高权限接口。
- **访问控制原则**：验证码发送/验证端点为公开端点（无需已认证身份），但必须完成人机验证（Turnstile）和限流；自动注册在验证成功且同意表达后创建账户。管理员启停配置复用既有 Realm Settings 权限（`settings.view`/`settings.manage`）。
- **租户/realm 数据边界**：请求绑定 Realm（与 Client App 上下文一致）；用户匹配和账户创建限定在当前 Realm 内；跨 Realm 数据访问被拒绝。
- **客户端会话方向**：登录成功签发 Bearer access/refresh token，承接自建用户 UI 的会话方向与权限上限；OTP 不引入新的会话模型。
- **未认证身份端点防护**：人机验证（Turnstile）按当前请求绑定的 Client App 的配置执行（Client App 级，见 D-PROTECT-01）；维持 IP/identifier 限流，不新增 client 维度限流。
- **兼容性**：与现有密码登录、TOTP、Passkey、协议同意和 Realm 注册政策共存；不绕过既有二因素或高危操作重新认证。

> 端点清单、参数 schema、状态码矩阵与验证码存储细节不在 PRD 承载范围，下沉到技术设计。

---

## 7. 前端/交互约束

**适用性**: 适用

- **页面入口**：登录页新增"邮箱验证码登录"入口，与密码登录并列；入口可见性由 Realm 是否启用决定。
- **关键交互**：
  - 用户选择"邮箱验证码登录" → 输入邮箱 → 完成人机验证（若启用） → 对未注册邮箱表达同意（"同意协议并继续"）→ 发送验证码 → 用户在邮箱收到验证码 → 输入验证码 → 验证成功登录（已注册）或自动注册并登录（未注册）。
- **状态反馈**：
  - 验证码错误/过期/重复使用 → 可区分的错误提示。
  - 发送频率受限 → 提示"请稍后再试"。
  - 账号被禁用 → 明确禁用提示。
  - 自动注册前未表达同意 → 阻止发送验证码并提示需先同意协议。
  - Realm 不允许自动注册 → 未注册邮箱得到未注册提示或显式注册引导。
  - 邮件延迟/未收到 → 提供"重新发送"（受频率限制）和密码登录回退。
- **降级**：Realm 未启用邮箱验证码登录 → 入口不展示，用户使用密码/TOTP/Passkey 登录。
- **权限可见性**：Realm 管理员的启停配置入口仅对拥有相应 Settings 权限的管理员可见。

> 前端实际承载方（ai-agent-app 或集成方自建 UI）由客户端会话方向决定；交互细节由前端设计承接。

---

## 8. 已确认决策

- **D-SCOPE-01（首期范围 Reduce）**：首期只对 ai-agent-app 对应 Realm 开放 OTP 登录与自动注册；不默认向所有 Realm 开启自动注册；推广由首期真实数据决定。
- **D-REG-01（未注册邮箱自动注册）**：验证成功后自动创建激活账户；自动注册视为注册路径而非登录异常分支。
- **D-EMAIL-01（邮箱仍必需）**：邮箱仍是账户必填身份与恢复渠道；验证码完成邮箱所有权验证；不引入无邮箱账户。
- **D-PASSKEY-01（Passkey 定位）**：Passkey 作为增强/未来主入口，不作为唯一入口；OTP 与密码仍需存在；后续按采用率调整曝光。
- **D-SESSION-01（会话方向承接）**：登录成功签发的会话承接自建用户 UI 的 Bearer access/refresh token 方向；不退回 cookie-only；自建用户 UI Bearer 登录能力须先于或同期交付。
- **D-CONSENT-01（自动注册的有效同意）**：用户输入邮箱 → 表达同意（勾选/点击"同意协议并继续"）→ 发送验证码 → 验证成功即视作对当前生效协议版本的有效同意并被记录；复用合规适配 PRD 的"登录即同意"语义。
- **D-PROTECT-01（防滥用，Client App 级 Turnstile）**：人机验证（Turnstile）配置在 **Client App 级别**——每个 Client App 配置自己的 Turnstile site_key/secret_key 与启用开关，`realm_config` 不再承载 Turnstile；OTP 登录/自动注册的人机验证按当前请求绑定的 Client App 的 Turnstile 配置执行。维持 IP/identifier 限流；验证码对发送频率、尝试次数、有效期和一次性消费设定上限；首期限定目标 Realm 控制成本。Client App 级 Turnstile 配置能力本身不在本 PRD 定义（由 Client App PRD 承载），本 PRD 只声明依赖。该下放对 Realm Settings PRD 与自建用户 UI PRD 的 Turnstile 表述构成同步治理约束，已一并修订（见 [docs/prd/core/realm-settings.md](../core/realm-settings.md)、[docs/prd/integration/custom-user-ui.md](../integration/custom-user-ui.md)）。
- **D-COEXIST-01（与其他认证方式共存）**：保留密码登录入口；OTP 不替代现有 TOTP 二因素或高危操作重新认证；用户可随后设置密码或绑定 Passkey/TOTP。

---

## 9. 参考资料

- 会话方向承接：[docs/prd/integration/custom-user-ui.md](../integration/custom-user-ui.md)（Bearer access/refresh token）
- 协议同意模型：[docs/prd/core/legal-consent-account-deletion.md](../core/legal-consent-account-deletion.md)（注册即同意 / 登录即同意）
- 用户管理 PRD：[docs/prd/core/users.md](../core/users.md)（账户状态枚举、注册政策）
- Realm 设置 PRD：[docs/prd/core/realm-settings.md](../core/realm-settings.md)（per-Realm 配置）
- Client App PRD：[docs/prd/integration/client-app.md](../integration/client-app.md)（Client App 级 Turnstile 配置）
- Passkey PRD：[docs/prd/auth/passkey.md](passkey.md)（Passkey 作为增强/未来主入口）
- 用户故事：[docs/user-stories/auth/email-otp-login.md](../../user-stories/auth/email-otp-login.md)
- 既有可引用用户故事：`docs/user-stories/core/regular-user.md`、`docs/user-stories/auth/passkey.md`、`docs/user-stories/core/legal-consent-account-deletion.md`
- 角色定义：[docs/user-stories/_roles.md](../../user-stories/_roles.md)
