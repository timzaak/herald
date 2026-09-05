# Realm 自定义域名（Custom Domain）产品需求文档 (PRD)

**创建时间**: 2026-07-10
**优先级**: P1

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/core/realm-custom-domain.md`。

### 1.1 相关故事

- `[US-CD-001]` 为本 Realm 配置自定义登录域名 (P0)，来源 `docs/user-stories/core/realm-custom-domain.md`
  - 角色：Realm Admin
  - 摘要：在管理后台为本 Realm 配置自定义登录域名（如 `login.acme.com`），获取 CNAME 指引并查看生效状态，域名全局唯一

- `[US-CD-003]` 自定义域名配置的授权门控 (P1)，来源 `docs/user-stories/core/realm-custom-domain.md`
  - 角色：Regular User
  - 摘要：自定义域名配置保存后立即生效（写入域名注册映射），授权反代层签发 TLS 证书

- `[US-CD-005]` 未授权域名访问的拒绝 (P1)，来源 `docs/user-stories/core/realm-custom-domain.md`
  - 角色：Regular User
  - 摘要：未在任意 Realm 注册的域名不会被 Herald 授权签发证书，防止证书滥用与钓鱼

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 1 | 配置自定义域名（CNAME 指引、全局唯一、生效状态） |
| P1 | 2 | 授权门控（已保存域名获 TLS 授权）、未授权域名拒绝 |
| P2 | 0 | - |

---

## 2. 范围界定

### 2.1 包含功能

- Per-realm 自定义登录域名配置（Realm Admin 在管理后台填域名，如 `login.acme.com`）
- 域名全局唯一约束（同一精确域名不可被多个 Realm 同时占用）
- 精确域名匹配（本期仅支持精确域名，不支持多级 / 通配域名）
- 系统向 Realm Admin 提供 CNAME 指引（CNAME 到 Herald 指定的 hostname）
- 域名生效状态对 Realm Admin 可见（CNAME 是否正确指向、TLS 是否就绪）
- 自定义域名配置单次保存即生效（填写 hostname 并保存后立即写入域名注册映射，无需单独发布步骤）
- 域名注册与证书授权的统一映射（已保存生效的自定义域名映射到对应 Realm）
- 证书滥用防护门控：Herald 仅对已在某 Realm 注册（经认证 Realm Admin 操作）的域名授权签发 TLS 证书；未注册域名不授权签发

### 2.2 不包含功能 (Out of Scope)

- Herald 通配子域 `*.herald.com`（仍含 herald.com，验证不足信任假设；用户已否决）
- 单独的 DNS TXT 所有权验证 UI 流程（CNAME + ACME 即所有权验证）
- 自定义域名的多级 / 通配支持（如 `*.acme.com`，本期仅精确域名匹配）
- 邮件发信域定制（SPF / DKIM / DMARC，属 realm-email-config，与登录页域名是两件事）
- 自定义域名资产审核 / 内容安全审核体系
- 自定义域名 TLS 证书的运维可见性面板（签发/续期状态面板，为未来范围）
- 父子 realm 层级（parent_id 继承、子 realm 继承父域名/配置等）：本期每个 realm 为平等租户，不引入父子层级，不修改 realm 数据模型

### 2.3 未来范围（Deferred）

以下能力原属本 feature 的完整设想，但因实现层面的技术约束（框架层面的请求改写层在路由匹配之后执行，无法在请求到达时按域名重写路径）尚未交付，列为未来范围，不在当前已发布能力中：

- **自定义域名下承载 auth 流（host→realm 解析）**：请求到达生效的自定义域名时，系统按域名映射到对应 Realm 并进入其 auth 流，URL 保持自定义域名（无需 `/{realmId}` path 前缀）。当前 realm 维度仍由 path 段 `{realmId}` 唯一决定，自定义域名暂不改变 realm 解析方式。
- **自定义域名覆盖全部 auth 流页面**：登录、注册、忘记密码、邮箱验证、OAuth 同意、TOTP、passkey 等 auth 流页面在自定义域名下提供。
- **自定义域名与 canonical `herald.com/{realmId}` 入口并存**（自定义域名作为额外入口与 path-based 入口同时可用）。
- **自定义域名与 canonical 域的跨域会话共享**（当前两个域的 session 不共享，为未来范围）。
- **动态 CORS**（按已解析 Realm 的域做动态 origin 校验）。

> 上述未来范围的落地依赖先解决框架层面的 host→realm 请求解析机制。当前已交付的配置生命周期与证书授权门控为其预留了映射基础。

> **已交付更新**：OAuth 回调 URL 已按自定义域名生成（`realm_public_origin_for_oauth` 优先返回生效的自定义域名，authorize/callback 的默认 `redirect_uri` 按其拼接）。该能力属 IdP 注册的安全敏感字段，已从未来范围移出。
>
> **已交付的 lookup helper（区别于上述框架层路由改写）**：`GET /api/public-config/custom-domain/resolve?host=<hostname>` 是一个公开、无认证的查询端点，附带按访问事实自报告 CNAME/TLS 展示状态（当请求 Host 与解析结果一致且经 https 访问时，顺带将映射的 cname_verified / tls_ready 展示状态置为 true；此为 tls_ready 置位的唯一途径，不影响授权判定与路由），供前端 SPA 在自定义域名入口处查表确定目标 realmId + publicConfig，不改变后端路由匹配或 path 解析方式。它不属于上述「框架层 host→realm 路由改写」未来范围，而是为其预留的映射基础的可读出口。

### 2.4 依赖项

- **Realm 系统** — 自定义域名属 Realm 级别，依赖 Realm 基础设施与 Realm 隔离
- **权限管理系统** — 自定义域名配置要求 `settings.manage` 权限（读要求 `settings.view`，与现有 Realm Config 一致）
- **White-label** — 自定义域名配置沿用 white-label 的存储与管理端模式（单 hostname 配置项，realm_config 存储）
- **现有反向代理层（生产 Caddy）** — 每域 TLS 自动化（ACME 签发/续期）依赖现有 Caddy 反代启用 On-Demand TLS，并在签发前询问 Herald 该 hostname 是否已注册并生效；Herald Rust app 不承担 TLS 终止
- **现有 Realm Config 配置模式** — 自定义域名配置沿用现有 Realm Config 的存储与管理端模式

---

## 3. 需求概述

### 3.1 功能描述

让 Herald 租户能够在本 Realm 配置一个完全自有的品牌登录域名（如 `login.acme.com`），为后续在该自有品牌域名下承载 auth 流提供配置与证书授权基础。本期交付配置能力与证书滥用防护：Realm Admin 配置自定义域名（保存即生效）、获取 CNAME 指引、查看生效状态；Herald 仅对已注册并生效的域名授权反代层签发 TLS 证书，未注册域名不被授权签发，防止证书滥用与钓鱼。

动机为**终端用户信任**——终端用户在 herald.com 承载的登录页可能因 URL 非租户品牌而犹豫/担心钓鱼，导致登录转化偏低；让租户能用自有品牌域名承载 auth 流，以验证"域名本身是独立信任因素"这一假设。当前已交付的配置与证书授权门控是该信任假设落地的前置基础。

### 3.2 关键特性

- Per-realm 自定义登录域名配置（精确域名匹配，全局唯一）
- CNAME + ACME 即所有权验证（无单独 DNS TXT 步骤）
- 每域 TLS 自动签发由现有 Caddy 反代承担（Herald app 不终止 TLS）
- 域名生效状态对 Realm Admin 可见（CNAME / TLS 是否就绪）
- 单次保存即生效（无需草稿/发布两步；自定义域名为极低频配置，且 hostname 必须 CNAME/TLS 验证才真正生效，三态生命周期无必要）
- 证书授权门控：仅已注册并生效的域名被授权签发 TLS，未注册域名拒绝

---

## 4. 业务规则与状态

### 4.1 业务规则

- **Realm 隔离**：自定义域名属于 Realm 级别，一个自定义域名关联唯一一个 Realm；不同 Realm 的自定义域名配置相互独立
- **域名全局唯一**：同一精确域名不可被多个 Realm 同时占用；后配置的 Realm 在保存时被拒绝并提示域名已被占用
- **精确域名匹配**：本期仅支持精确域名匹配（如 `login.acme.com`），不支持多级/通配域名（如 `*.acme.com`）
- **域名规范化**：hostname 服务端强制小写化、去尾点、拒绝含协议/端口/路径/通配的输入，防止通过大小写或尾点绕过唯一约束
- **权限要求**：仅 Realm Admin（持有 `settings.manage` 写 / `settings.view` 读）可查看和配置本 Realm 的自定义域名；Regular User 无配置入口
- **所有权验证（简化 BYO）**：自定义域名无需单独 DNS TXT 所有权验证步骤；CNAME 到 Herald 指定 hostname 即隐含 DNS 控制权证明，ACME 签发挑战本身进一步验证 DNS 实际指向 Herald，二者组合等价于所有权验证
- **TLS 自动签发**：Herald 为每个已配置且 CNAME 正确生效的自定义域授权签发 TLS 证书（ACME）；Herald Rust app 不承担 TLS 终止，TLS 由现有 Caddy 反代层承担
- **未授权域名拒绝**：Herald 仅对已在某 Realm 注册（经认证 Realm Admin 操作）并已保存生效的域名授权签发 TLS；未注册域名不授权签发证书，防止证书滥用与钓鱼
- **生效模型**：Realm Admin 保存自定义域名配置后立即生效（写入域名注册映射 + settings 配置项）；清空 hostname 保存则移除映射，域名不再生效
- **保存副作用**：保存自定义域名配置时，系统把该 hostname 写入域名注册映射（供证书授权门控查询）；清空 hostname 保存则移除该映射
- **证书授权门控查询基准**：证书授权门控仅以"已配置且启用"为生效判定；CNAME 是否已验证、TLS 是否已就绪是面向 Realm Admin 的展示态信息，不纳入授权判定（否则授权与签发形成循环）

### 4.2 关键状态与异常

- **域名生效状态可见**：Realm Admin 在配置端可见每个已配置域名的生效状态（CNAME 是否已正确指向 Herald、TLS 是否已就绪），以便自助排查未生效原因
- **CNAME 未正确指向**：域名已配置但 CNAME 尚未正确指向 Herald 时，该域名 TLS 未就绪；配置端状态显示为未生效
- **未注册域名访问**：未在任何 Realm 注册的域名，Herald 不授权签发 TLS 证书
- **配置错误恢复**：配置了错误自定义域名时，Realm Admin 可直接清空 hostname 或填入正确域名重新保存，立即覆盖生效

---

## 5. 功能需求

### 5.1 核心需求

- **自定义域名配置**：Realm Admin 可在管理后台为本 Realm 配置一个自定义登录域名（精确域名，全局唯一）
- **CNAME 指引**：系统向 Realm Admin 展示需要 CNAME 到的 Herald 指定 hostname，并展示域名生效状态（CNAME 是否正确指向、TLS 是否就绪）
- **域名生效状态可见**：Realm Admin 可查看每个已配置域名的当前生效状态
- **单次保存即生效**：自定义域名配置无草稿/发布两步流程，保存即写入域名注册映射并生效
- **未授权域名拒绝**：未注册域名不被授权签发证书
- **配置隔离与权限**：自定义域名按 Realm 隔离，仅 Realm Admin 可配置

### 5.2 验收目标

- Realm Admin 能在管理后台 Settings 页面为本 Realm 成功配置并保存自定义登录域名，域名被全局唯一约束保护
- Realm Admin 能看到 CNAME 指引与域名生效状态（CNAME / TLS 是否就绪）
- 尝试配置已被其他 Realm 占用的域名时，系统拒绝并提示域名已被占用
- 自定义域名保存后，该 hostname 立即被写入域名注册映射；证书授权门控据此授权
- 未在任何 Realm 注册的域名不会被 Herald 授权签发 TLS 证书
- 仅 Realm Admin 可见和操作自定义域名配置入口

---

## 6. API 相关约束

**适用性**: 适用

- **接口能力范围**：
  - 管理端：自定义域名配置的读取、保存（Realm Admin），沿用现有 Realm Config 管理端模式
  - 证书授权查询：供现有 Caddy 反代 On-Demand TLS 在签发前询问 Herald 该 hostname 是否已注册并生效，兼作证书滥用门控
- **访问控制原则**：管理端读写要求 Realm Admin（`settings.view` 读 / `settings.manage` 写）并通过 Realm 归属校验；证书授权查询供反代层内部调用（访问控制由技术设计定义）
- **数据边界原则**：自定义域名配置按 Realm 隔离，不同 Realm 之间不可交叉访问；域名全局唯一约束跨 Realm 生效
- **证书授权门控响应边界**：授权查询仅返回是否授权的结论，不泄露 Realm 身份或其他信息
- **不引入资产上传接口**：本期不涉及资产上传
- 详细接口契约、错误模型与反代层授权查询契约在技术设计文档中维护

---

## 7. 前端/交互约束

**适用性**: 适用

- **配置页入口**：管理后台 Settings 页面新增「自定义域名」配置入口（与现有 Turnstile / Registration / Email / TOTP / Passkey / 品牌化等 Tab 同级），置于 canonical 域管理后台（`herald.com/{realmId}/manage/settings`），realmId 从 UI 上下文获取
- **配置表单**：包含自定义域名输入字段，支持保存即生效（无草稿/发布两步流程）
- **CNAME 指引与状态反馈**：配置端向 Realm Admin 展示需要 CNAME 到的 Herald 指定 hostname，并展示域名生效状态（CNAME 是否正确指向、TLS 是否就绪）
- **域名全局唯一校验**：输入已被其他 Realm 占用的域名时，前端/后端拒绝并提示域名已被占用
- **角色差异**：仅 Realm Admin 可见和操作「自定义域名」配置入口；Regular User 无配置入口

---

## 8. 已确认决策

### 8.1 来自 Decision Brief 的已确认决策（D0 / D1）

- **动机归类（D0）**：终端用户信任（非 B2B 成单、非邮件可达性、非纯品类对标）；成功标准以「信任/转化提升」衡量，不以「能力完整」衡量
- **范围形态（D0）**：简化 BYO 自定义域（CNAME + ACME，无单独 DNS TXT 验证），非 Herald 通配子域、非完整审核体系
- **致命假设（D0）**：域名是终端用户信任的瓶颈（white-label UI 品牌化可能已解决信任）；Kill Criteria——上线 3 个月 A/B 验证转化，无提升则降级/收缩
- **简化策略（D0）**：无单独 DNS TXT 验证步骤，CNAME + ACME 即所有权验证
- **TLS 自动化（D1）**：ACME 每域签发，由现有 Caddy 反代承担（技术预研确认）

### 8.2 PRD 阶段承接/确认的决策

- **TLS 落点确认**：Herald Rust app 保持纯 TCP listener，不承担 TLS 终止；每域 TLS 终止与 ACME 签发/续期由现有 Caddy 反代承担；不引入任何新库
- **简化 BYO 安全性确认**：证书滥用风险通过「反代层授权查询 + ACME DNS 控制证明」组合消解，无需加回 DNS TXT 验证步骤
- **自定义域名配置沿用 white-label 存储模式但简化为单次保存**：自定义域名配置复用 white-label 的 realm_config 存储与管理端模式，但不采用其 draft/publish/restore 三态生命周期——自定义域名为极低频配置（基本只在初次接入时配置），且 hostname 必须 CNAME/TLS 验证才真正生效，三态生命周期无必要，改为单次保存即生效。
- **hostname 全局唯一**：域名注册映射以 hostname 为全局唯一键，保证同一精确域名不被多个 Realm 同时占用；存储与表结构细节由技术设计定义，不在 PRD 范围
- **证书授权门控查询基准**：授权判定仅以"已配置且启用"为准；CNAME/TLS 状态为展示态字段，不纳入授权判定，避免授权与签发形成循环
- **扁平 realm，无父子层级**：用户所说"各个子 realm"即现有扁平 realm；本期每个 realm 为平等租户，不引入 parent_id 父子层级，不修改 realm 数据模型

### 8.3 与既有设计文档的关系（Rule 7）

- **与 passkey 设计文档的冲突（已按自定义域名侧落地）**：passkey 设计文档假设「单一部署统一域名，所有 Realm 共享 RP」与自定义域名的 per-realm RP 需求直接冲突。当前 passkey RP 按请求 Origin 与域映射（custom_domain_mapping / client_app allowed_origins）派生 per-realm / per-app RP ID，并校验其归属 realm；canonical 与自定义域凭证不互通，单一域名假设不再成立。此为显式冲突标记，不静默合并。
- host→realm 路由解析在上一版实现后曾被回退（根因：框架层面改写层在路由匹配之后执行，无法改写 URI）。该重建属技术实现，当前列为未来范围，不在本期已发布能力中。

### 8.4 与 Decision Brief 的关系（Rule 7）

- Decision Brief 中的 Possible Expansions / Open Questions（跨域会话共享、TLS 运维面板）在本 PRD §2.2 / §2.3 中明确列为 Out of Scope 或未来范围，未作为已确认决策写入。
- ACME 每域 TLS 管道在 Decision Brief 阶段为「阻塞 Proceed」的 Open Question，经技术预研确认可行（有条件：反代层授权查询 + 无新库）后已转为 §8.2 已确认决策。

---

## 9. 参考资料

- 用户故事：`docs/user-stories/core/realm-custom-domain.md`
- 相关 PRD：`docs/prd/core/ui-custom.md`（white-label PRD，本能力的配置范式来源）
- 相关 PRD：`docs/prd/core/realm-settings.md`（Realm Config 配置模式基线）
- 相关 PRD：`docs/prd/core/realm.md`
- 角色定义：`docs/user-stories/_roles.md`
