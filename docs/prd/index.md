# PRD 文档索引

本文档索引列出所有 Herald 系统的产品需求文档（PRD）。

## 按能力包阅读（推荐）

PRD 文件保留独立主题，便于评审和追踪；规划、排期和端到端评审时，按下列能力包合并理解，避免把同一用户旅程拆散。

| 能力包 | 目标 | 应合并阅读的 PRD |
|-------|------|------------------|
| 租户与运营 | 创建、配置和运营 Realm | [Realm](core/realm.md)、[SaaS 自助注册开通 Realm](core/realm-create.md)、[Realm Settings](core/realm-settings.md)、[Dashboard](core/dashboard.md)、[Audit](core/audit.md) |
| 用户生命周期与合规 | 用户从注册、资料维护到协议确认和注销 | [Users](core/users.md)、[会话管理/强制下线](core/kickoff-user.md)、[合规适配](core/legal-consent-account-deletion.md) |
| 登录体验与品牌 | 提供统一、可品牌化且可本地化的认证入口 | [OAuth](auth/oauth.md)、[微信 OAuth](auth/wechat-oauth.md)、[邮箱验证码登录](auth/email-otp-login.md)、[Google One Tap](auth/google-one-tap.md)、[Apple native 登录](auth/support-mobile-apple-login.md)、[LDAP 企业目录登录](auth/support-ldap.md)、[White-label](core/ui-custom.md)、[自定义域名](core/realm-custom-domain.md)、[i18n](core/i18n.md) |
| 强认证 | 配置并完成多因素或无密码认证 | [TOTP](auth/totp.md)、[Passkey](auth/passkey.md)、[Device Code](auth/device-code.md) |
| 授权与应用接入 | 管理 RBAC、Client App、API Key 与 SDK 接入 | [权限管理](auth/permissions.md)、[Client App](integration/client-app.md)、[API Key 角色](integration/api-key-roles.md)、[SDK](integration/sdk.md)、[JS 浏览器 SDK](integration/js-sdk.md)、[自建用户 UI](integration/custom-user-ui.md) |
| 商品、支付与权益履约 | 从商品同步、购买到订阅/权益生效和异常补偿 | [Subscription（含多价格、产品同步与 Webhook 补偿）](billing/subscription.md)、[履约模型扩展（买断与非续期订阅）](billing/pay_model.md)、[Stripe 支付](billing/stripe-payment.md)、[Paywall](billing/support-paywall.md)、[App Store / Google Play 内购(IAP)](billing/support-iap.md)、[WeChat Pay 支持](billing/wechat-support.md)、[多货币购买体验](billing/multiple-currency.md) |
| 余额与财务凭证 | 管理积分账户，以及支付对应的发票和贷记凭证 | [积分](billing/points.md)、[积分账户](billing/credit-bucket.md)、[多钱包积分分发规则](billing/multi-wallet-grant-rules.md)、[发票（含支付归属与 Credit Note）](billing/invoice.md) |

能力包是导航和评审边界，不是新增需求，也不取代各 PRD。只有当两个文件描述同一业务对象、同一生命周期且不能独立交付时，才应进一步物理合并。

## 文档组织结构

```
docs/
├── prd/                    # 产品需求文档（Product Requirements）
│   ├── index.md           # 本文件 - PRD 全局索引
│   ├── core/              # 核心功能（Realm、用户、审计等）
│   ├── auth/              # 认证与授权（OAuth、TOTP、权限等）
│   ├── billing/           # 计费与订阅（订阅、积分、支付等）
│   └── integration/       # 集成与扩展（Client App、SDK 等）
└── user-stories/           # 用户故事（User Stories）
    ├── index.md           # 用户故事索引
    ├── core/              # 核心功能用户故事
    ├── auth/              # 认证授权用户故事
    ├── billing/           # 计费相关用户故事
    └── integration/       # 集成相关用户故事
```

## PRD 文档列表

### Core 核心功能

| PRD 文档 | 标题 | 相关角色 |
|---------|------|---------|
| [realm.md](core/realm.md) | Realm 管理 | Admin Realm, Realm Admin |
| [realm-create.md](core/realm-create.md) | SaaS 自助注册开通 Realm | SaaS 自助注册访客, Admin Realm, Realm Admin |
| [users.md](core/users.md) | 用户管理 | Realm Admin, Regular User |
| [kickoff-user.md](core/kickoff-user.md) | 会话管理 / 强制用户下线（查看与撤销用户活跃会话 + Forbidden 联动下线） | Realm Admin |
| [realm-settings.md](core/realm-settings.md) | Realm 设置 | Realm Admin |
| [ui-custom.md](core/ui-custom.md) | White-label（Per-Realm 登录/注册及 Auth 流 UI 定制） | Realm Admin, Regular User |
| [realm-custom-domain.md](core/realm-custom-domain.md) | Realm 自定义域名（配置 + 证书授权门控） | Realm Admin, Regular User |
| [audit.md](core/audit.md) | Audit 审计日志 | Realm Admin, Admin Realm |
| [dashboard.md](core/dashboard.md) | Dashboard | Realm Admin |
| [i18n.md](core/i18n.md) | 国际化（i18n）支持 | All Users |
| [legal-consent-account-deletion.md](core/legal-consent-account-deletion.md) | 合规适配（用户协议 / 隐私政策 / 账户注销） | Regular User, Realm Admin |

### Auth 认证与授权

| PRD 文档 | 标题 | 相关角色 |
|---------|------|---------|
| [oauth.md](auth/oauth.md) | OAuth 与第三方集成 | Realm Admin, Regular User, Third-Party App |
| [wechat-oauth.md](auth/wechat-oauth.md) | 微信 OAuth 集成 | Realm Admin, Regular User |
| [email-otp-login.md](auth/email-otp-login.md) | 邮箱验证码登录（含未注册邮箱自动注册） | Regular User, Realm Admin |
| [google-one-tap.md](auth/google-one-tap.md) | Google One Tap 登录（第三方页面无跳转 Google 登录） | Regular User, Third-Party App |
| [support-mobile-apple-login.md](auth/support-mobile-apple-login.md) | 苹果手机 App native 登录（iOS App 内无跳转 Apple 登录） | Regular User, Third-Party App |
| [support-ldap.md](auth/support-ldap.md) | LDAP 企业目录登录（企业账号密码 + 首登自动建号） | Realm Admin, Regular User |
| [totp.md](auth/totp.md) | TOTP 二次认证 | TOTP User, Realm Admin |
| [passkey.md](auth/passkey.md) | Passkey 认证（无密码第一因素 + 第二因素） | Regular User, Realm Admin |
| [permissions.md](auth/permissions.md) | 权限管理 | Realm Admin |
| [device-code.md](auth/device-code.md) | Device Code 登录 | Third-Party App, Regular User, Realm Admin |

### Billing 计费与订阅

| PRD 文档 | 标题 | 相关角色 |
|---------|------|---------|
| [subscription.md](billing/subscription.md) | 订阅计费、Entitlement 映射、Webhook 处理（含 One-time 购买） | Realm Admin, Regular User, Third-Party App, System |
| [pay_model.md](billing/pay_model.md) | 履约模型扩展：买断（一次性购买 + 永久角色）与非续期订阅 | Realm Admin, Regular User, Third-Party App, System |
| [support-paywall.md](billing/support-paywall.md) | 支付驱动权益门控（role 授予横切维度、支付成功自动授权、订阅过期自动撤销、一人一次防重复） | Realm Admin, Regular User, Third-Party App, System |
| [points.md](billing/points.md) | 积分系统（含发放、免费用户积分、发放时序与可用性） | Realm Admin, Regular User, Third-Party App |
| [credit-bucket.md](billing/credit-bucket.md) | 积分账户（余额池与 Client App 覆盖范围） | Realm Admin, Regular User, Third-Party App |
| [multi-wallet-grant-rules.md](billing/multi-wallet-grant-rules.md) | 多钱包积分分发规则（一次购买/注册多账户扇出） | Realm Admin, Regular User, Third-Party App, System |
| [stripe-payment.md](billing/stripe-payment.md) | Stripe 支付集成 | Realm Admin |
| [support-iap.md](billing/support-iap.md) | App Store / Google Play 内购(IAP) 支持 | Realm Admin, Third-Party App, System |
| [wechat-support.md](billing/wechat-support.md) | WeChat Pay 支持（PC 扫码 Native 与微信内 JSAPI） | Realm Admin, Regular User, System |
| [multiple-currency.md](billing/multiple-currency.md) | 多货币（按货币选择/本地化购买体验） | Realm Admin, Regular User, Third-Party App |
| [invoice.md](billing/invoice.md) | Invoice 发票管理（含 Provider 发票同步和自研 Fallback） | Realm Admin, Regular User |

### Integration 集成与扩展

| PRD 文档 | 标题 | 相关角色 |
|---------|------|---------|
| [client-app.md](integration/client-app.md) | Client App 管理 | Realm Admin, Third-Party App |
| [sdk.md](integration/sdk.md) | SDK 资源管理 | Third-Party App |
| [api-key-roles.md](integration/api-key-roles.md) | API Key 角色绑定 | Realm Admin |
| [custom-user-ui.md](integration/custom-user-ui.md) | 自建用户 UI（跨域 Bearer token + 双轨凭证类，集成方自建全套终端用户 UI） | Third-Party App, Regular User |
| [js-sdk.md](integration/js-sdk.md) | JS 浏览器 SDK（第三方网页集成，官方浏览器认证生命周期封装） | Third-Party App |

## 相关文档

- **用户故事索引**: [docs/user-stories/index.md](/docs/user-stories/index.md)

## PRD 分层约束

- PRD 只承载业务范围、规则、约束、验收目标与必要的交互边界。
- PRD 不承载接口端点清单、请求响应 schema、状态码矩阵、数据库建表/迁移细节或代码类型定义。
- 详细接口契约、数据库结构和实现方案应下沉到技术设计、接口说明和代码。
