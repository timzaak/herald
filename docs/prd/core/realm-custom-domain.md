# Realm 自定义域名（Custom Domain）产品需求文档 (PRD)

**创建时间**: 2026-07-10
**优先级**: P1

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/core/realm-custom-domain.md`。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| US-CD-001 | 为本 Realm 配置自定义登录域名 | P0 | `docs/user-stories/core/realm-custom-domain.md` |
| US-CD-003 | 自定义域名配置的授权门控 | P1 | `docs/user-stories/core/realm-custom-domain.md` |
| US-CD-005 | 未授权域名访问的拒绝 | P1 | `docs/user-stories/core/realm-custom-domain.md` |

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

前端 SPA 已通过公开 resolve 接口解析请求 Host，支持已绑定自定义域名下的无 realm 前缀 auth、manage、user 页面；主域名同时保留 `/{realmId}/...` 入口。父路由统一确认 realm 后再执行认证与子守卫，不依赖服务端框架的路径改写。

以下能力仍为未来范围：

- **自定义域名与 canonical 域的跨域会话共享**（当前两个域的 session 不共享，为未来范围）。
- **动态 CORS**（按已解析 Realm 的域做动态 origin 校验）。

> 自定义域名和主域名属于不同 origin，当前各自恢复会话；域名解析不等于跨域登录状态共享。

> **按自定义域名生成的链接**：覆盖 OAuth 回调与全部身份邮件/设备码链接面——`realm_public_origin_for_oauth` 优先返回生效的自定义域名，authorize/callback 的默认 `redirect_uri` 按其拼接；注册验证邮件链接、邮箱验证、更换邮箱、找回/重置密码与设备码授权页链接（`realm_public_url_parts`/`realm_public_url`）同样优先落在生效的自定义域名上。
>
> **lookup helper（区别于上述框架层路由改写）**：`GET /api/public-config/custom-domain/resolve` 是一个公开、无认证的查询端点，仅以请求自身的 `Host` 头（去端口后按域名规范化规则归一）为查表键——不存在 `?host=` 覆盖参数（任意 host→realm 探测只能走共享密钥保护的 Caddy ask 端点），Host 头缺失或未命中返回 404。展示状态的自报告路径受信：仅当请求的套接字对端属于部署配置的 `trusted_proxies`（操作者声明的 TLS 终接 ingress）且其转发的 `X-Forwarded-Proto: https` 时，才顺带将映射的 `cname_verified` / `tls_ready` 展示状态置为 true（直连或未受信对端伪造的转发头不生效；此为 tls_ready 置位的唯一途径，不影响授权判定与路由）。响应体按 Host 选择（租户维度），固定携带 `Cache-Control: no-store` 与 `Vary: Host`，防止 URL 键控的共享缓存跨 Host 回放他租户配置。供前端 SPA 在自定义域名入口处查表确定目标 realmId + publicConfig；它不属于上述「框架层 host→realm 路由改写」未来范围，而是为其预留的映射基础的可读出口。

### 2.4 依赖项

- **Realm 系统** — 自定义域名属 Realm 级别，依赖 Realm 基础设施与 Realm 隔离
- **权限管理系统** — 自定义域名配置要求 `settings.manage` 权限（读要求 `settings.view`，与现有 Realm Config 一致）
- **White-label** — 自定义域名配置沿用 white-label 的存储与管理端模式（单 hostname 配置项，realm_config 存储）
- **现有反向代理层（生产 Caddy）** — 每域 TLS 自动化（ACME 签发/续期）依赖现有 Caddy 反代启用 On-Demand TLS，并在签发前询问 Herald 该 hostname 是否已注册并生效；Herald Rust app 不承担 TLS 终止
- **现有 Realm Config 配置模式** — 自定义域名配置沿用现有 Realm Config 的存储与管理端模式

---

## 4. 业务规则与状态

### 4.1 业务规则

- **Realm 隔离**：自定义域名属于 Realm 级别，一个自定义域名关联唯一一个 Realm；不同 Realm 的自定义域名配置相互独立
- **域名全局唯一**：同一精确域名不可被多个 Realm 同时占用；后配置的 Realm 在保存时被拒绝并提示域名已被占用
- **部署保留域名拒绝**：`localhost`、部署 `public_base_url` 主机与 Herald CNAME target 主机是平台基础设施名，租户 claim 这些域名时被拒绝（防止把平台宿主的 realm 解析/白标面/OIDC issuer 重定向到租户 realm）；部署 URL 配置畸形时 fail-closed 禁止全部 claim
- **精确域名匹配**：本期仅支持精确域名匹配（如 `login.acme.com`），不支持多级/通配域名（如 `*.acme.com`）
- **域名规范化**：hostname 服务端强制小写化、去尾点、拒绝含协议/端口/路径/通配的输入，防止通过大小写或尾点绕过唯一约束；随后经 UTS #46 IDNA 严格模式归一化为 Punycode A-label 存储——Unicode 原文（U-label）与既有 Punycode 拼写视为同一域名，跨 Realm 冲突判定按归一化后的存储值比较
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

## 5. 验收目标

- Realm Admin 能在管理后台 Settings 页面为本 Realm 成功配置并保存自定义登录域名，域名被全局唯一约束保护
- Realm Admin 能看到 CNAME 指引与域名生效状态（CNAME / TLS 是否就绪）
- 尝试配置已被其他 Realm 占用的域名时，系统拒绝并提示域名已被占用
- 自定义域名保存后，该 hostname 立即被写入域名注册映射；证书授权门控据此授权
- 未在任何 Realm 注册的域名不会被 Herald 授权签发 TLS 证书
- 仅 Realm Admin 可见和操作自定义域名配置入口

---

## 6. 边界与约束

**适用性**: 适用（API 与前端/交互边界合并陈述）

**API / 集成边界:**
- **接口能力范围**：管理端为自定义域名配置的读取、保存（Realm Admin），沿用现有 Realm Config 管理端模式；证书授权查询供现有 Caddy 反代 On-Demand TLS 在签发前询问 Herald 该 hostname 是否已注册并生效，兼作证书滥用门控
- **访问控制边界**：管理端权限要求见 §4.1「权限要求」，读写均通过 Realm 归属校验；证书授权查询供反代层内部调用（访问控制由技术设计定义）
- **证书授权门控响应边界**：授权查询仅返回是否授权的结论，不泄露 Realm 身份或其他信息
- 不引入资产上传接口：本期不涉及资产上传
- 详细接口契约、错误模型与反代层授权查询契约在技术设计文档中维护

**前端 / 交互边界:**
- **配置页入口**：管理后台 Settings 页面新增「自定义域名」配置入口（与现有 Turnstile / Registration / Email / TOTP / Passkey / 品牌化等 Tab 同级），置于 canonical 域管理后台（`herald.com/{realmId}/manage/settings`），realmId 从 UI 上下文获取
- **配置表单**：包含自定义域名输入字段，支持保存即生效（无草稿/发布两步流程）
- **角色差异**：仅 Realm Admin 可见和操作「自定义域名」配置入口；Regular User 无配置入口

---

## 7. 已确认决策

**来自 Decision Brief 的已确认决策（D0 / D1）:**

- **动机归类（D0）**：终端用户信任（非 B2B 成单、非邮件可达性、非纯品类对标）；成功标准以「信任/转化提升」衡量，不以「能力完整」衡量
- **范围形态（D0）**：简化 BYO 自定义域（CNAME + ACME，无单独 DNS TXT 验证），非 Herald 通配子域、非完整审核体系
- **致命假设（D0）**：域名是终端用户信任的瓶颈（white-label UI 品牌化可能已解决信任）；Kill Criteria——上线 3 个月 A/B 验证转化，无提升则降级/收缩
- **简化策略（D0）**：无单独 DNS TXT 验证步骤，CNAME + ACME 即所有权验证
- **TLS 自动化（D1）**：ACME 每域签发，由现有 Caddy 反代承担（技术预研确认）

**PRD 阶段承接/确认的决策:**

- **TLS 落点确认**：Herald Rust app 保持纯 TCP listener，不承担 TLS 终止；每域 TLS 终止与 ACME 签发/续期由现有 Caddy 反代承担；不引入任何新库
- **简化 BYO 安全性确认**：证书滥用风险通过「反代层授权查询 + ACME DNS 控制证明」组合消解，无需加回 DNS TXT 验证步骤
- **自定义域名配置沿用 white-label 存储模式但简化为单次保存**：自定义域名配置复用 white-label 的 realm_config 存储与管理端模式，但不采用其 draft/publish/restore 三态生命周期——自定义域名为极低频配置（基本只在初次接入时配置），且 hostname 必须 CNAME/TLS 验证才真正生效，三态生命周期无必要，改为单次保存即生效。
- **hostname 全局唯一**：域名注册映射以 hostname 为全局唯一键，保证同一精确域名不被多个 Realm 同时占用；存储与表结构细节由技术设计定义，不在 PRD 范围
- **证书授权门控查询基准**：授权判定仅以"已配置且启用"为准；CNAME/TLS 状态为展示态字段，不纳入授权判定，避免授权与签发形成循环
- **扁平 realm，无父子层级**：用户所说"各个子 realm"即现有扁平 realm；本期每个 realm 为平等租户，不引入 parent_id 父子层级，不修改 realm 数据模型

**与既有设计文档的关系:**

- **与 passkey 的 RP 边界**：passkey RP 按请求 Origin 与域映射（custom_domain_mapping / client_app allowed_origins）派生 per-realm / per-app RP ID，并校验其归属 realm；canonical 与自定义域凭证不互通。
- host→realm 路由解析属技术实现，当前属未来范围（框架层面改写层在路由匹配之后执行，无法改写 URI）。

---

## 8. 参考资料

- 相关 PRD：`docs/prd/core/ui-custom.md`（white-label PRD，本能力的配置范式来源）
- 相关 PRD：`docs/prd/core/realm-settings.md`（Realm Config 配置模式基线）
- 相关 PRD：`docs/prd/core/realm.md`
- 角色定义：`docs/user-stories/_roles.md`
- 用户故事来源见 §1 表格
