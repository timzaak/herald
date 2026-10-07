# MCP Server 产品需求文档 (PRD)

**创建时间**: 2026-08-31
**优先级**: P1

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

| US-ID | 标题 | 优先级 | 来源 |
|-------|------|--------|------|
| US-MCP-001 | 把 Herald 接入 AI agent 客户端（浏览器授权） | P0 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-002 | 通过 agent 查询用户 | P1 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-003 | 通过 agent 查询积分余额 | P1 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-004 | 通过 agent 查询积分交易流水 | P1 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-005 | 通过 agent 查询审计日志 | P1 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-006 | 通过 agent 查询 Realm 配置状态 | P1 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-007 | 通过 agent 查询我的资料 | P1 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-008 | 通过 agent 查询我的积分余额 | P1 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-009 | 通过 agent 查询我的积分交易流水 | P1 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-010 | 通过 agent 查询我的订阅 | P1 | `docs/user-stories/integration/mcp-server.md` |
| US-MCP-011 | 查看与关闭本租户 MCP 接入 | P1 | `docs/user-stories/integration/mcp-server.md` |

`/mcp` 不消费 Client API Key 体系，对 US-RA-016/017/018（API Key 角色管理）无依赖；该体系自身语义不变（`docs/prd/integration/api-key-roles.md`）。

---

## 2. 范围界定

### 2.1 包含功能

- 面向 AI agent 客户端的 Herald MCP 服务：一个公网可接入的 MCP 协议端点，支持 agent 客户端完成连接、获取工具清单、调用工具的完整链路
- **OAuth 用户凭证接入（resource server 模式）**：`/mcp` 是 Herald 自身 OAuth 体系的受保护资源——未认证请求返回规范要求的授权挑战（含授权服务器发现入口，RFC 9728），MCP 客户端经发现 → 浏览器授权码 + PKCE 登录授权 → 凭证交换完成接入；访问凭证代表完成授权的 Herald 用户
- **授权流扩展（复用既有授权服务器，见 `docs/prd/auth/oauth.md` 基线）**：授权请求接受 MCP 资源标识（RFC 8707 resource indicator）并端到端绑定到签发凭证；授权服务器元数据补齐 OAuth 授权服务器能力声明；凭证端点提供标准刷新授权类型，支持 MCP 客户端长会话续期
- **每 realm 预置一个内置 MCP 公共客户端**（PKCE-only；回调白名单覆盖 MCP 客户端的 loopback 回调模式），接入目标为仅填写服务地址、无需创建凭证；客户端身份经**受控 pass-through 动态注册端点**取得（DEC-mcp-server-007）：授权服务器元数据声明 RFC 7591 `registration_endpoint`，该端点在通过校验（限流 + 规范请求 + loopback 回调形状）后返回预置客户端身份，不创建任何注册状态。客户端取得预注册身份的完整链路以 §5 的真实客户端验证为准
- **管理面五项只读查询能力**：
  1. 查询用户（列表与详情）
  2. 查询指定用户的积分余额
  3. 查询指定用户的积分交易流水
  4. 查询本租户审计日志
  5. 查询本租户配置状态概览
- **self 面四项只读查询能力**：我的资料、我的积分余额、我的积分交易流水、我的订阅与权益状态——操作对象一律从授权身份推导
- MCP 专用授权 scope 集：self 面读能力的许可范围门控；不含管理面权限与任何写权限
- 严格的租户（realm）数据隔离：所有工具仅返回授权用户所属租户的数据（self 面天然只返回本人数据）
- 主流 agent 客户端（Claude Code、Cursor、VS Code 等）的 OAuth 接入文档（含各客户端配置示例、浏览器授权说明与连通性自检）与官方文档站 MCP 集成指南
- 管理台 Client App 列表对内置 MCP 客户端的展示与「系统内置」标识（不可删除）

### 2.2 不包含功能 (Out of Scope)

- **写操作工具**（创建用户、发放积分、修改配置等）：不提供；扩展时按逐工具安全评审准入（DEC-mcp-server-004 确立的既定路径）
- **开放式动态客户端注册**（任意调用方可注册并获得独立客户端凭据）与 **RFC 7592 客户端管理**：不做。仅实现返回预置 `herald-mcp` 公共客户端身份的受控 pass-through 注册端点，无注册状态、无独立凭据、无管理协议（DEC-mcp-server-007）
- **Client API Key 在 /mcp 的兼容保留或过渡双轨**：完全移除（DEC-mcp-server-003，破坏性变更）；Herald 其余 API Key 能力面（ext API）不受影响
- **token JWT 化与 JWKS 验证路径**：访问凭证维持不透明形态、由服务端本地验证，不做自包含凭证
- **用户主动授权/拒绝授权确认页**：沿用既有「登录即完成授权」的授权模型（`docs/prd/auth/oauth.md` §2.2），MCP 授权不新增确认页面
- **MCP Auth 等价物**（客户的 MCP server 用 Herald 做鉴权）：属另一产品方向且无需求证据，不做
- 管理后台新增独立管理页面（内置 MCP 客户端仅呈现于既有 Client App 列表）
- 新增 RBAC 权限定义或内置角色（管理面复用既有 `resource.action` 权限体系）
- 专用独立部署形态（MCP 端点与现有服务同进程暴露）

### 2.3 依赖项

以下均为既有能力，本功能零新增外部依赖：

- 既有授权码 + PKCE 授权服务器：授权、凭证交换、state 与授权码一次性、redirect 白名单校验
- 既有浏览器 token 家族生命周期：轮换刷新、复用检测吊销、绝对上限；共享 Bearer 验证中间件（含 client_app 与用户状态复核）
- 既有用户主体 RBAC（`check_principal_permission`）与 realm 隔离校验
- 既有 Client App 实体、内置应用保护与禁用级联规则（`docs/prd/integration/client-app.md`）
- 既有登录页与登录同意闸门（浏览器授权交互直接复用，无新页面）
- 既有服务限流与安全中间件栈；rmcp MCP 协议栈（服务端授权行为自建于 transport 外侧）
- 官方文档站与面向 agent 的文档索引

---

## 4. 业务规则与状态

### 4.1 业务规则

- **工具边界**：管理面五项 + self 面四项，共九项只读查询能力；不提供任何修改、删除或创建类工具
- **鉴权规则**：`/mcp` 仅接受 Herald 自身签发、audience 绑定 MCP 资源的 OAuth 用户凭证；Client API Key 的两种传输形态（`X-API-Key` 头与按 API Key 解释的 Bearer）一律拒绝，不进入鉴权路径。凭证缺失或无效时返回规范要求的授权挑战与授权服务器发现入口，MCP 客户端据此引导用户完成浏览器授权
- **授权流程**：MCP 客户端经授权服务器发现发起授权码 + PKCE（S256）流程；用户在 Herald 既有登录页完成认证（含登录同意闸门），登录完成即授权完成；state 与授权码一次性使用
- **audience 隔离（RFC 8707）**：授权请求显式携带 MCP 资源标识，签发凭证内嵌 audience 并在 `/mcp` 验证时强匹配；既有浏览器会话凭证（未携带 MCP 资源标识）不得访问 `/mcp`，MCP 凭证亦不得访问管理台/用户中心的凭证面——两个凭证面双向不通用
- **凭证生命周期**：MCP 凭证沿用既有 token 家族模型（轮换、复用检测吊销、绝对上限），对客户端保持不透明；支持标准刷新授权类型，长会话可续期；授权用户被禁用或强制下线、内置 MCP 客户端被禁用时，名下 MCP 凭证即时失效
- **权限规则（管理面）**：每项管理面工具映射到授权用户的既有查看权限（用户查看、积分查看、审计查看、设置查看），不额外要求 self 面 scope；用户未持对应权限时拒绝读取并告知需由管理员授予对应角色权限，重新登录或扩大 self scope 均不能取得管理权限
- **self 面规则**：self 工具的操作对象一律为授权用户本人，不接受指定其他用户；对应读能力的 MCP scope 是唯一能力许可门控，任何 Herald 用户持有所需 scope 即可读取本人数据，无需管理员授予查询角色；scope 不足时拒绝读取并提供客户端可识别的授权挑战与重新授权所需范围。MCP scope 集不含管理面权限与写权限
- **租户边界**：所有工具仅能访问授权用户所属租户的数据；查询目标不在所属租户时表现为资源不存在，不返回任何数据
- **校验顺序**：鉴权（含 audience 匹配）、权限/scope、取数三道校验按序执行——先鉴权、再权限、后取数
- **积分余额口径**：管理面积分余额工具返回指定用户在 Realm 范围内的余额；self 面返回授权用户本人的余额
- **内置 MCP 客户端**：每个 realm 预置一个，PKCE-only 公共客户端，回调仅允许已登记的 loopback 地址；支持 IPv4/IPv6 loopback IP 与客户端兼容所需的 localhost，允许动态端口，但回调协议、主机及路径仍须匹配登记值，不允许任意路径或非 loopback 地址。loopback 回调可使用 HTTP，其余授权通信使用 HTTPS；属系统内置、不可删除；禁用即吊销名下全部 MCP 凭证并拒绝新授权，构成本 realm 关闭 MCP 接入的开关；重新启用不恢复既有授权，agent 须重新完成浏览器授权
- **MCP 专用限流**：每个授权用户的 MCP 请求量受独立的限流约束，超限明确拒绝；限流与具体工具的业务权限检查互相独立
- **时间筛选格式**：积分交易与审计工具统一接受 RFC 3339 时间戳，或按 UTC 零点解释的 `YYYY-MM-DD`
- **数据最小化**：工具返回字段以回答业务问题所需为限，不透出多余字段，降低查询结果被 agent 带出至第三方模型的数据面
- **错误语义**：未找到、参数错误以工具级、agent 可读的错误返回；管理面 RBAC 不足明确指引角色授权，不诱导重复登录；self scope 不足通过授权协议错误与挑战指引重新授权，不能仅返回工具文案。所有拒绝均不返回查询数据，不向 agent 暴露内部错误细节
- **用量与观测**：只读阶段不新增审计事件；服务级可观测手段沿用
- **协议兼容**：实现 MCP 现行稳定规范（2026-07-28）的 OAuth 分支要求（受保护资源元数据发现、loopback 回调、PKCE）；客户端注册以受控 pass-through DCR 端点满足（返回预置公共客户端，兼容以 DCR 为接入前提的主流客户端，DEC-mcp-server-007）
- **演进约束**：写操作工具进入前必须逐工具通过安全评审，评审要素含最小权限映射、审计覆盖、排除不可逆/破坏性操作、批量上限、限流约束与返回字段最小化

### 4.2 关键状态与异常

- **接入异常**：未完成授权、凭证无效或已被吊销（token 家族吊销、授权用户被禁用/强制下线、内置 MCP 客户端被禁用）均在连接/调用入口以授权挑战拒绝，agent 客户端引导用户（重新）完成浏览器授权
- **调用异常**：权限不足或 scope 不足、资源不存在（含跨租户目标）、参数校验失败均返回对应的 agent 可读错误，不返回部分数据
- **audience 错配**：浏览器会话凭证访问 `/mcp`、MCP 凭证访问其他凭证面，均被拒绝——凭证面之间不互通
- **兼容性状态（破坏性变更）**：Client API Key 已从 `/mcp` 移除，无过渡双轨；以 API Key 接入 `/mcp` 的旧使用方全部失效，接入文档、场景测试与示例均为 OAuth 路线
- **验证期状态（Wedge）**：首发后 4-6 周观察期；若无 agent 使用信号、无社区反馈增长，则停止扩展写操作，降级为实验特性或下线（既定 Kill Criteria）。认证与工具面的后续修订不重置该观察期、不撤销去留条件

---

## 5. 验收目标

- 主流 MCP 客户端满足仅填写服务地址的接入目标，无需创建凭证或手工补填客户端身份：经浏览器授权完成连接，并列出全部九项查询能力
- 接入验收以真实客户端全链路实测为准：使用真实 MCP 客户端（至少 Claude Code 与 VS Code）的全新配置，仅填写对应租户的 MCP 服务地址，自动取得预注册客户端身份，经浏览器授权后能发现并调用查询能力，并记录客户端版本、配置、预注册信息取得方式和实际回调地址；需要手工补填客户端身份、未获准回调或无法完成授权均不满足零配置验收
- 管理面工具按授权用户的 RBAC 正确放行与拒绝：持对应权限的用户调用每项工具均返回正确的租户数据；未持权限时明确拒绝并指引角色授权，不受 self scope 变更影响
- self 面工具返回授权用户本人的资料、积分余额、流水与订阅，且不接受指定其他用户；无管理权限的普通用户持有所需 scope 即可成功查询，scope 不足时拒绝并通过授权挑战引导补足范围
- audience 隔离双向生效：浏览器会话凭证不能访问 `/mcp`，MCP 凭证不能访问管理台/用户中心凭证面
- Client API Key 的两种形态（`X-API-Key` 头、按 API Key 解释的 Bearer）在 `/mcp` 均被拒绝，无双轨残留
- 长会话经标准刷新授权类型续期不断线；凭证吊销（用户禁用/强制下线、内置客户端禁用）即时生效
- 内置 MCP 客户端在管理台 Client App 列表可见并标注系统内置、不可删除；禁用后该 realm 的 MCP 接入关闭
- Live Demo 环境端到端可用：真实 agent 客户端完成 OAuth 接入并成功执行查询
- 官方接入文档与文档站 MCP 集成指南同步为 OAuth 路线（含各客户端配置示例与浏览器授权说明）
- 按 §4.2 的观察期与去留条件评估产品使用信号

---

## 6. 边界与约束

**适用性**: API/集成边界与前端/交互边界均适用（内置客户端展示与浏览器授权体验）。

**API / 集成边界:**

- **能力范围**：仅查询类工具能力（九项，见 §2.1）；授权能力面含受保护资源元数据发现（RFC 9728）、授权码 + PKCE 签发（resource 参数扩展）、标准刷新授权类型、受控 pass-through 客户端注册端点（RFC 7591 形状，返回预置身份）；无写入、回调或推送类能力
- **访问控制**：OAuth 用户凭证（audience 绑定 MCP）+ 管理面既有 `resource.action` 权限检查 / self 面 MCP scope 门控 + 租户隔离（校验顺序见 §4.1）
- **授权服务器关系与例外范围**：以 `docs/prd/auth/oauth.md` 为基线，MCP 侧补充 resource/audience 绑定、标准刷新授权类型、授权服务器元数据与受保护资源发现；另对内置 MCP 公共客户端解释 self 读 scope，并允许 §4.1 的 HTTP loopback 动态端口回调。这些例外仅适用于该客户端及 MCP 授权；其他客户端的非 openid scope 透传、生产 HTTPS 回调与端口精确匹配规则继续按发布基线执行
- **协议边界**：MCP 端点是独立于既有 REST 管理 API 的协议面，不进入 REST 接口文档（OpenAPI）体系；协议版本兼容性与具体端点形态、工具命名、入参结构、scope 命名不在本 PRD 定义，归技术设计阶段
- **接口说明来源**：授权集成规则参考 [MCP Authorization Specification](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)，注册规则参考 [Client Registration](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/client-registration) 与 RFC 7591（仅注册请求/响应形状，不含开放注册语义）；工具清单与调用语义通过工具发现向 agent 自描述，人类接入说明由官方 MCP 集成指南承载
- **兼容性**：Client API Key 从 `/mcp` 移除是破坏性变更，无过渡双轨；Herald 其余 API Key 面（ext API，`docs/prd/integration/api-key-roles.md`）语义不受影响

**前端 / 交互边界:**

- 管理台 Client App 列表：内置 MCP 公共客户端随列表可见，带「系统内置」标识，不允许删除（与既有内置应用保护一致）；禁用沿用既有 Client App 操作交互
- 浏览器授权体验：由 MCP 客户端在授权挑战后拉起浏览器完成，复用 Herald 既有登录页与登录同意闸门，无新增页面或授权确认页
- 本功能面向开发者的可见面为接入文档与连通性自检说明（官方文档站 + README）；agent 客户端内的工具交互由 agent 客户端自身呈现，Herald 不控制其 UI

---

## 7. 已确认决策

> 本节只收录当前有效的决策与未决问题，记取舍、理由、决策人与重开条件；规则正文只在 §4 定义，DEC/Q 编号保持稳定，供代码注释、测试与跨 PRD 引用追溯。

- **DEC-mcp-server-003 · mcp.auth-oauth**（user）：`/mcp` 认证采用 Herald 自身 OAuth 签发的用户凭证（resource server 模式：未认证请求返回授权挑战与 RFC 9728 受保护资源元数据发现、RFC 8707 resource/audience 端到端绑定、凭证维持不透明形态本地验证、支持标准刷新授权类型）；Client API Key 路径完全移除（破坏性变更，接入方/测试/文档同步迁移，不留双轨）。取代 DEC-mcp-server-002（MCP 复用 Client API Key、OAuth 列为演进路径——其重开条件「用户要求浏览器授权 UX」被触发）。理由：用户要求浏览器授权 UX；竞品调研（GitHub 官方远程 MCP、Logto、WorkOS、Auth0）表明 per-user OAuth 是远程 MCP server 的行业默认模式。落点：§2.1、§2.2、§4.1、§4.2、§5、§6。重开条件：MCP 客户端生态对本路线兼容性实测失败且不可修复。
- **DEC-mcp-server-004 · mcp.tool-surface**（user）：工具面 = 管理面 5 只读工具（查用户/积分余额/流水/审计/配置状态）保留，权限映射为用户主体 RBAC；新增 self 面只读工具（我的资料/积分/流水/订阅，userId 从身份推导、不接受参数指定）；整体只读，写操作不在当前范围。取代 DEC-mcp-server-001（首发只读工具集——其「只读 + 每工具一权限 + 写操作走逐工具安全评审」原则被本决策继承；管理面查询能力经验证有保留价值）。理由：用户裁决增加 self 面（承接「用户经 MCP 看到自己内容」的需求方向与竞品 per-user 模式）。落点：§2.1、§2.2、§4.1、§5。重开条件：写操作准入评审时（沿用逐工具安全评审）。
- **DEC-mcp-server-007 · mcp.client-bootstrap**（user）：AS 元数据声明 `registration_endpoint`，实现受控 pass-through DCR——注册端点经校验（per-IP 限流 + 规范 RFC 7591 请求 + redirect_uris 全为 loopback /callback 模板形状）后返回预置 `herald-mcp` 公共客户端（PKCE-only、无 secret、无注册状态）；开放式 DCR 与 RFC 7592 客户端管理不做。取代 DEC-mcp-server-005（预置客户端、不做 DCR——其重开条件「客户端生态必须 DCR 才能接入」于 2026-10-06 实测触发：Claude Code 拒连无 registration_endpoint 的 AS，VS Code/Cursor 同为 DCR 优先）。理由：用户裁决「受控 pass-through DCR」；Figma 先例证明该形态可兼顾零配置与滥用面控制，pass-through 无状态使滥用面与预置现状等价。落点：§2.1、§2.2、§4.1、§5、§6。重开条件：pass-through 形态被主流客户端拒绝或注册端点被证实可滥用。

---

## 8. 参考资料

- 技术预研：`.ai/tech-research/mcp-server.md`
- 技术设计：`.ai/design/mcp-server.md`
- 相关已发布 PRD：`docs/prd/auth/oauth.md`（授权服务器基线与授权模型）、`docs/prd/auth/openid-connect.md`（well-known 元数据端点先例）、`docs/prd/integration/client-app.md`（内置应用保护与禁用级联）、`docs/prd/auth/permissions.md`（RBAC）、`docs/prd/integration/api-key-roles.md`（API Key 面，不受本功能影响）、`docs/prd/billing/points.md`（积分系统）、`docs/prd/core/audit.md`（审计日志）
- 官方 MCP 集成指南：https://www.fornetcode.com/en/docs/integration/mcp （中文：https://www.fornetcode.com/zh/docs/integration/mcp ）
- MCP Authorization Specification（2026-07-28）：https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization
- RFC 9728（Protected Resource Metadata）、RFC 8707（Resource Indicators）、RFC 8252（Loopback Redirect）
- 用户故事来源见 §1 表格
