# 微信 OAuth 集成产品需求文档 (PRD)

**创建时间**: 2026-03-03
**优先级**: P1

---
## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 故事引用

**租户管理员**
- `[US-RA-010]` OAuth Provider 配置管理 (P0)，来源 `docs/user-stories/auth/oauth-extension.md`
  - 角色：Realm Admin
  - 摘要：管理 OAuth Provider 配置（Google、GitHub、Facebook、Apple、WeChat），以便用户可以使用第三方登录

**租户用户**
- `[US-RU-003]` OAuth 第三方登录 (P1)，来源 `docs/user-stories/core/regular-user.md`
  - 角色：Regular User
  - 摘要：使用第三方账号（Google、GitHub、Facebook、Apple、WeChat）登录，以便无需记忆额外密码

**微信专属**
- `[US-RA-011]` WeChat OAuth Provider 配置 (P1)，来源 `docs/user-stories/auth/wechat-oauth.md`
  - 角色：Realm Admin
  - 摘要：配置 WeChat OAuth Provider，以便用户可以使用微信登录
- `[US-RA-012]` WeChat Mini Program Provider 配置 (P1)，来源 `docs/user-stories/auth/wechat-oauth.md`
  - 角色：Realm Admin
  - 摘要：配置 WeChat Mini Program Provider，以便小程序用户可以登录
- `[US-RU-010]` 微信网站应用登录 (P1)，来源 `docs/user-stories/auth/wechat-oauth.md`
  - 角色：Regular User
  - 摘要：使用微信扫码登录，以便快速访问系统
- `[US-RU-011]` 微信小程序登录 (P1)，来源 `docs/user-stories/auth/wechat-oauth.md`
  - 角色：小程序用户
  - 摘要：使用微信账号登录，以便在小程序内访问 Herald 服务

### 1.2 优先级汇总表

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 1 | OAuth Provider 配置管理 |
| P1 | 4 | WeChat OAuth Provider 配置、WeChat Mini Program Provider 配置、微信网站应用登录、微信小程序登录 |
| P2 | 0 | - |

---
## 2. 范围界定

### 2.1 包含功能
- 网站应用微信登录（QRconnect，PC 网站扫码登录）
- 微信小程序登录（code2session）
- WeChat OAuth Provider 配置管理
- WeChat Mini Program Provider 配置管理
- UnionID 机制支持（跨应用用户匹配）
- Placeholder 邮箱生成（微信不提供邮箱）

### 2.2 不包含功能 (Out of Scope)
- 微信公众号登录（需要不同的 OAuth 流程）
- 微信支付集成（不涉及支付功能）
- 微信社交分享功能
- 微信用户信息解密（如需敏感信息，需要额外开发）

### 2.3 依赖项
- OAuth 2.0 框架（支持 Google、GitHub、Facebook、Apple）
- Realm 隔离机制

---
## 3. 需求概述

### 3.1 功能描述
Herald 项目需要接入微信账号体系，支持两种登录方式：
1. **网站应用微信登录** - PC 网站用户扫码登录
2. **微信小程序登录** - 小程序内使用微信账号登录

这两种登录方式使用不同的 API 流程，但需要统一的用户管理和 UnionID 匹配机制。

### 3.2 关键特性
- **UnionID 机制**: 支持跨应用用户匹配，同一开放平台账号下的用户使用相同的 UnionID
- **Placeholder 邮箱**: 微信不提供邮箱，自动生成占位符邮箱
- **配置灵活**: Realm Admin 可以独立配置 WeChat 和 WeChat Mini Program Provider

---
## 4. 业务规则与状态

### 4.1 业务规则

**UnionID 机制**
- 同一开放平台账号下的所有应用，用户的 UnionID 相同
- 优先使用 UnionID 进行用户匹配
- UnionID 获取条件：应用必须绑定到微信开放平台
- 支持网站应用和小程序之间的跨应用用户匹配

**Scope 配置**
- 网站应用微信登录的 scope 必须是 `snsapi_login`（固定值）
- WeChat Mini Program Provider 不需要配置 scope
- UnionID 通过访问用户信息接口获取（网站应用），不是在 scope 中指定

**Email 处理**
- 微信不提供邮箱地址
- Placeholder 邮箱生成策略：
  - 优先使用: `{unionid}@wechat.placeholder`（如果 unionid 可用）
  - 降级使用: `{openid}@wechat.placeholder`（如果 unionid 不可用）
- 邮箱标记为可选（verified: false）
- 在需要邮箱的场景提示用户补充真实邮箱

**用户匹配优先级**（实现为四级：unionid → openid → email → 创建）
1. 优先通过 unionid 查找已存在的用户
2. 如果找不到，通过 openid 查找
3. 如果还找不到，通过 email 查找（占位邮箱 `{unionid/openid}@wechat.placeholder` 以 `verified=false` 落库，实际微信登录永不依赖 email 匹配成功，占位仅为满足唯一约束）
4. 如果还找不到，创建新用户（自动注册受 Realm 注册政策门控：注册关闭时不自动建号，按登录失败处理并引导）
5. OAuth 自动创建用户后触发统一注册后事件，可按 Realm 的积分分发规则发放注册积分

**跨应用匹配**
- 使用 UnionID 作为跨应用匹配的唯一标识
- 同一用户可以有多个 Provider 记录（wechat、wechat-miniprogram），UnionID 相同

**与其他 OAuth Provider 的区别**

- WeChat Mini Program 不生成浏览器授权 URL，必须调用专用小程序登录端点；传给通用 `/{provider}/login` 入口会返回 400

| 特性 | Google/GitHub/Facebook/Apple | WeChat (网站应用) | WeChat Mini Program |
|------|----------------------------|-------------------|---------------------|
| OAuth 流程 | 标准 OAuth 2.0 | 三步法流程 | code2session（非标准） |
| Scope | 可配置多种 scopes | 固定 `snsapi_login` | 不需要 scope |
| UnionID | 无 | 支持（需绑定开放平台） | 支持（需绑定开放平台） |
| Email 提供 | 提供真实邮箱 | Placeholder 邮箱 | Placeholder 邮箱 |
| QR Code | 不需要 | 需要扫码 | 不需要 |

### 4.2 关键状态与异常

**安全约束**
- state 参数防止 CSRF 攻击（网站应用）
- code 只能使用一次，10 分钟内有效（网站应用）
- js_code 只能使用一次，5 分钟内有效（小程序）
- Client Secret 不在 GET 响应中返回
- 编辑模式下 Client Secret 为可选（留空表示不更新）
- 所有 Provider 配置操作记录审计日志

---
## 5. 功能需求

### 5.1 核心需求

**网站应用微信登录**
- 用户点击"微信登录"按钮后，系统生成授权 URL 并重定向到微信授权页面
- 用户扫码并授权后，微信回调到 Herald 系统并携带 code
- 系统使用 code 换取 access_token，再获取用户信息（含 unionid）
- 系统创建或匹配用户并设置 session
- 专用微信授权 URL 入口支持透传已有 `downstream_state`，用于第三方 Client App 的下游 Code+PKCE 授权事务

**微信小程序登录**
- 小程序用户触发微信登录，系统接收小程序发送的授权码
- 系统验证授权码并获取用户信息，创建或匹配用户
- 返回访问令牌给小程序用户

**Realm Admin 配置管理**
- WeChat OAuth Provider 配置项：Client ID（AppID）、Client Secret（AppSecret）、Scope（固定 `snsapi_login`）、Enabled；Redirect URI 无需在 Herald 侧配置存储，回调地址运行时由部署域名/自定义域名派生（租户需在微信开放平台侧登记该回调地址）
- WeChat Mini Program Provider 配置项：Client ID（AppID）、Client Secret（AppSecret）、Enabled
- 操作：创建、查看、编辑、删除、启用/禁用
- WeChat OAuth Provider 的 Scope 为固定值，不可修改；Mini Program Provider 不需要配置 scope 和 redirect_uri

### 5.2 验收目标
- 网站应用微信扫码登录全流程可正常完成
- 微信小程序登录全流程可正常完成
- UnionID 跨应用匹配正确工作
- Placeholder 邮箱按规则生成
- Realm Admin 可独立配置和管理两种 Provider
- 所有安全约束得到满足

---
## 6. API 相关约束

**适用性**: 适用

- 仅说明认证、授权、验证、回调或账号绑定等能力边界，不在 PRD 中展开端点、请求响应 schema、状态码矩阵
- 必须遵守 realm 隔离、权限边界、凭证脱敏和幂等要求；涉及回调时需满足回调来源校验、重放防护和错误可恢复性
- 若存在第三方身份提供商回调，应在技术设计或接口说明中维护详细契约，PRD 只保留业务约束和兼容性要求

---
## 7. 前端/交互约束

**适用性**: 适用

- 仅保留页面入口、关键用户路径、状态反馈、权限可见性和异常提示要求，不写组件实现步骤或前端类型定义
- 认证相关流程应优先保证成功/失败状态清晰、回跳路径明确、敏感信息不回显，并对首次配置、失效、锁定、重试等场景提供稳定反馈

---
## 8. 已确认决策

### 8.1 已确认决策
- 使用 UnionID 作为跨应用用户匹配的唯一标识
- 微信不提供邮箱，采用 Placeholder 邮箱策略
- WeChat OAuth 和 WeChat Mini Program 作为两个独立 Provider 配置

---
## 9. 参考资料
- 用户故事：`docs/user-stories/auth/oauth-extension.md`
- 用户故事：`docs/user-stories/core/regular-user.md`
- 用户故事：`docs/user-stories/auth/wechat-oauth.md`
- 微信开放平台文档: https://open.weixin.qq.com/cgi-bin/showdocument?action=dir_list&t=resource/res_list&verify=1&id=open1419316505&token=&lang=zh_CN
- 微信小程序登录文档: https://developers.weixin.qq.com/miniprogram/dev/OpenApiDoc/user-info/phone-number/getPhoneNumber.html
