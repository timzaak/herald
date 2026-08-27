# 用户故事索引

本文档索引列出所有 Herald 系统的用户故事。

## 按用户旅程阅读（推荐）

下面按完整用户旅程组织阅读入口；后面的 US-ID 表用于定位具体可验收目标。

| 能力包 | 连续旅程 | 用户故事文件 |
|-------|----------|--------------|
| 租户与运营 | 创建 Realm → 配置 → Dashboard 运营 → 审计 | [Admin Realm](core/admin-realm.md)、[自助开通 Realm](core/realm-create.md)、[Realm Admin](core/realm-admin.md)、[审计](core/audit.md)、[内置保护](core/builtin-protection.md) |
| 用户生命周期与合规 | 注册/登录 → 资料维护 → 协议确认 → 账户注销 | [普通用户](core/regular-user.md)、[合规适配](core/legal-consent-account-deletion.md) |
| 登录体验与品牌 | 第三方登录 → 品牌页面 → 自定义域名 → 多语言 | [OAuth 应用](auth/third-party-app.md)、[OAuth 扩展](auth/oauth-extension.md)、[微信 OAuth](auth/wechat-oauth.md)、[邮箱验证码登录](auth/email-otp-login.md)、[Google One Tap](auth/google-one-tap.md)、[Apple native 登录](auth/support-mobile-apple-login.md)、[LDAP 企业目录登录](auth/support-ldap.md)、[White-label](core/white-label.md)、[自定义域名](core/realm-custom-domain.md)、[i18n](core/i18n.md) |
| 强认证 | 管理员配置 → 用户绑定 → 登录/恢复 | [TOTP](auth/totp.md)、[Passkey](auth/passkey.md)、[Device Code](auth/device-code.md) |
| 授权与应用接入 | RBAC 配置 → Client App/API Key → SDK 调用 → 自建用户 UI | [Realm Admin](core/realm-admin.md)、[Client App 设置](auth/client-app-settings.md)、[SDK](integration/sdk.md)、[JS 浏览器 SDK](integration/js-sdk.md)、[自建用户 UI](integration/custom-user-ui.md) |
| 商品、支付与权益履约 | Provider 配置/同步 → 下单 → 支付 → 订阅或权益生效 → 补偿 | [支付平台](billing/payment-provider.md)、[Entitlement Mapping](billing/entitlement-mapping.md)、[支付尝试](billing/payment-attempt.md)、[订阅](billing/subscription.md)、[履约模型扩展（买断/非续期订阅）](billing/pay_model.md)、[Paywall](billing/support-paywall.md)、[Webhook 补偿](billing/webhook-compensation.md)、[IAP](billing/support-iap.md)、[WeChat Pay](billing/wechat-support.md)、[多货币](billing/multiple-currency.md) |
| 积分与余额 | 策略配置 → 免费/付费发放 → 分账户持有和消费 → 查询历史 | [积分管理](billing/points-admin.md)、[免费积分](billing/points-free-user.md)、[积分包购买](billing/points-package-purchase.md)、[用户积分](billing/points-user.md)、[异步支付积分](billing/async-payment-points.md)、[积分账户](billing/credit-bucket.md)、[多钱包分发规则](billing/multi-wallet-grant-rules.md) |
| 发票与退款凭证 | 支付归属 → 发票同步/开具 → 退款和 Credit Note | [发票与 Provider Fallback](billing/invoice.md)、[支付-发票归属](billing/payment-invoice-mapping.md) |

### 故事边界

- 一个故事只表达一个可验收目标。
- 同一旅程中的配置、终端操作和系统履约可以放在同一能力包，但保留各自角色。
- Provider 差异优先写成同一故事的场景；只有用户目标或生命周期不同才拆故事。

## 文档层级

- 上面的能力包用于产品评审、端到端验收和排期，是默认阅读入口。
- 下方 US-ID 索引用于定位单个可验收目标，不作为产品目录重复维护。
- 物理目录 `core / auth / billing / integration` 只表示代码与文档归属，不再逐文件展开成第二套导航。

## 用户故事 ID 索引（追踪视图）

| US-ID | 标题 | 角色 | 优先级 | 文件 |
|-------|------|------|--------|------|
| US-AR-001 | 创建 Realm | Admin Realm | P0 | [core/admin-realm](core/admin-realm.md#故事-1创建-realm-us-ar-001) |
| US-AR-002 | 查看 Realm 列表 | Admin Realm | P0 | [core/admin-realm](core/admin-realm.md#故事-2查看-realm-列表-us-ar-002) |
| US-AR-003 | 查看 Realm 详情 | Admin Realm | P1 | [core/admin-realm](core/admin-realm.md#故事-3查看-realm-详情-us-ar-003) |
| US-AR-004 | Realm 创建权限控制 | Admin Realm | P0 | [core/admin-realm](core/admin-realm.md#故事-4realm-创建权限控制-us-ar-004) |
| US-AR-005 | 访问新创建的 Realm | Admin Realm | P0 | [core/admin-realm](core/admin-realm.md#故事-5访问新创建的-realm-us-ar-005) |
| US-SR-001 | 自助注册开通新 Realm | SaaS 自助注册访客 | P0 | [core/realm-create](core/realm-create.md#故事-1自助注册开通新-realm-us-sr-001) |
| US-SR-002 | 开通后立即管理新 Realm | 新 realm-admin | P0 | [core/realm-create](core/realm-create.md#故事-2开通后立即管理新-realm-us-sr-002) |
| US-SR-003 | Admin Realm 管理员查看自助开通的 Realm | Admin Realm | P1 | [core/realm-create](core/realm-create.md#故事-3admin-realm-管理员查看自助开通的-realm-us-sr-003) |
| US-SR-004 | 平台自助开通开关控制 | Admin Realm | P0 | [core/realm-create](core/realm-create.md#故事-4平台自助开通开关控制-us-sr-004) |
| US-RA-001 | Realm 隔离访问 | Realm Admin | P0 | [core/realm-admin](core/realm-admin.md#故事-1realm-隔离访问-us-ra-001) |
| US-RA-002 | 角色定义管理 | Realm Admin | P0 | [core/realm-admin](core/realm-admin.md#故事-2角色定义管理-us-ra-002) |
| US-RA-003 | 权限定义管理 | Realm Admin | P0 | [core/realm-admin](core/realm-admin.md#故事-3权限定义管理-us-ra-003) |
| US-RA-004 | 为角色分配权限 | Realm Admin | P0 | [core/realm-admin](core/realm-admin.md#故事-4为角色分配权限-us-ra-004) |
| US-RA-005 | 查看角色权限 | Realm Admin | P0 | [core/realm-admin](core/realm-admin.md#故事-5查看角色权限-us-ra-005) |
| US-RA-006 | 用户角色分配 | Realm Admin | P0 | [core/realm-admin](core/realm-admin.md#故事-6用户角色分配-us-ra-006) |
| US-RA-007 | 权限策略管理 | Realm Admin | P0 | [core/realm-admin](core/realm-admin.md#故事-7权限策略管理-us-ra-007) |
| US-RA-008 | 订阅套餐管理 | Realm Admin | P0 | [core/realm-admin](core/realm-admin.md#故事-8订阅套餐管理-us-ra-008) |
| US-RA-009 | 权限层级验证 | Realm Admin | P0 | [core/realm-admin](core/realm-admin.md#故事-9权限层级验证-us-ra-009) |
| US-BP-001 | 内置角色和权限保护 | Realm Admin | P0 | [core/builtin-protection](core/builtin-protection.md#故事-1内置角色和权限保护-us-bp-001) |
| US-RA-010 | 查看 Dashboard 用户活跃概览 | Realm Admin | P1 | [core/realm-admin](core/realm-admin.md#故事-10查看-dashboard-用户活跃概览-us-ra-010) |
| US-RA-011 | 查看 Dashboard 认证趋势图 | Realm Admin | P1 | [core/realm-admin](core/realm-admin.md#故事-11查看-dashboard-认证趋势图-us-ra-011) |
| US-RA-012 | 通过 Dashboard 快捷导航跳转 | Realm Admin | P1 | [core/realm-admin](core/realm-admin.md#故事-12通过-dashboard-快捷导航跳转-us-ra-012) |
| US-RA-013 | 配置 Realm 邮件服务 | Realm Admin | P0 | [core/realm-admin](core/realm-admin.md#故事-13配置-realm-邮件服务-us-ra-013) |
| US-RA-014 | 发送测试邮件 | Realm Admin | P1 | [core/realm-admin](core/realm-admin.md#故事-14发送测试邮件-us-ra-014) |
| US-RA-015 | 邮件依赖的功能开关前置验证 | Realm Admin | P0 | [core/realm-admin](core/realm-admin.md#故事-15邮件依赖的功能开关前置验证-us-ra-015) |
| US-RA-016 | API Key 角色管理 | Realm Admin | P0 | [core/realm-admin](core/realm-admin.md#故事-16api-key-角色管理-us-ra-016) |
| US-RA-017 | 创建 API Key 时绑定角色 | Realm Admin | P0 | [core/realm-admin](core/realm-admin.md#故事-17创建-api-key-时绑定角色-us-ra-017) |
| US-RA-018 | API Key 按 Client App 隔离 | Realm Admin | P0 | [core/realm-admin](core/realm-admin.md#故事-18api-key-按-client-app-隔离-us-ra-018) |
| US-RA-019 | 管理本 Realm 的用户协议与隐私政策 | Realm Admin | P1 | [core/legal-consent-account-deletion](core/legal-consent-account-deletion.md#故事-19管理本-realm-的用户协议与隐私政策-us-ra-019) |
| US-RA-020 | 查看并撤销指定用户的活跃会话 | Realm Admin | P1 | [core/realm-admin](core/realm-admin.md#故事-19查看并撤销指定用户的活跃会话-us-ra-020) |
| US-RA-021 | 禁用用户时即时撤销其全部活跃会话 | Realm Admin | P1 | [core/realm-admin](core/realm-admin.md#故事-20禁用用户时即时撤销其全部活跃会话-us-ra-021) |
| US-RU-001 | 账号注册 | Regular User | P0 | [core/regular-user](core/regular-user.md#故事-1账号注册-us-ru-001) |
| US-RU-002 | 账号登录 | Regular User | P0 | [core/regular-user](core/regular-user.md#故事-2账号登录-us-ru-002) |
| US-RU-003 | OAuth 第三方登录 | Regular User | P0 | [core/regular-user](core/regular-user.md#故事-3oauth-第三方登录-us-ru-003) |
| US-RU-004 | 修改个人密码 | Regular User | P1 | [core/regular-user](core/regular-user.md#故事-4修改个人密码-us-ru-004) |
| US-RU-005 | 查看个人资料 | Regular User | P1 | [core/regular-user](core/regular-user.md#故事-5查看个人资料-us-ru-005) |
| US-RU-006 | 修改个人昵称 | Regular User | P2 | [core/regular-user](core/regular-user.md#故事-6修改个人昵称-us-ru-006) |
| US-RU-007 | 退出登录 | Regular User | P1 | [core/regular-user](core/regular-user.md#故事-7退出登录-us-ru-007) |
| US-RU-008 | 访问第三方应用（SSO） | Regular User | P0 | [core/regular-user](core/regular-user.md#故事-8访问第三方应用-us-ru-008) |
| US-RU-009 | 认证重定向流程 | All Users | P0 | [core/regular-user](core/regular-user.md#故事-9认证重定向流程-us-ru-009) |
| US-RU-010 | 从第三方 Web 应用跳转登录 | Regular User | P0 | [core/regular-user](core/regular-user.md#故事-10从第三方-web-应用跳转登录-us-ru-010) |
| US-RU-011 | 注册时同意用户协议与隐私政策 | Regular User | P0 | [core/legal-consent-account-deletion](core/legal-consent-account-deletion.md#故事-11注册时同意用户协议与隐私政策-us-ru-011) |
| US-RU-012 | 协议更新后重新同意 | Regular User | P0 | [core/legal-consent-account-deletion](core/legal-consent-account-deletion.md#故事-12协议更新后重新同意-us-ru-012) |
| US-RU-013 | 查看当前生效的用户协议与隐私政策 | Regular User | P1 | [core/legal-consent-account-deletion](core/legal-consent-account-deletion.md#故事-13查看当前生效的用户协议与隐私政策-us-ru-013) |
| US-RU-014 | 自助注销账户（软删除） | Regular User | P0 | [core/legal-consent-account-deletion](core/legal-consent-account-deletion.md#故事-14自助注销账户软删除-us-ru-014) |
| US-RU-015 | 登录时确认同意用户协议与隐私政策 | Regular User | P0 | [core/legal-consent-account-deletion](core/legal-consent-account-deletion.md#故事-15登录时确认同意用户协议与隐私政策-us-ru-015) |
| US-WO-001 | WeChat OAuth Provider 配置 | Realm Admin | P1 | [auth/wechat-oauth](auth/wechat-oauth.md#故事-1wechat-oauth-provider-配置-us-wo-001) |
| US-WO-002 | WeChat Mini Program Provider 配置 | Realm Admin | P1 | [auth/wechat-oauth](auth/wechat-oauth.md#故事-2wechat-mini-program-provider-配置-us-wo-002) |
| US-WO-003 | 微信网站应用登录 | Regular User | P1 | [auth/wechat-oauth](auth/wechat-oauth.md#故事-3微信网站应用登录-us-wo-003) |
| US-WO-004 | 微信小程序登录 | Regular User | P1 | [auth/wechat-oauth](auth/wechat-oauth.md#故事-4微信小程序登录-us-wo-004) |
| US-TP-001 | OAuth 授权码登录 | Third-Party App | P0 | [auth/third-party-app](auth/third-party-app.md#故事-1oauth-授权码登录authorization-code-pkce-us-tp-001) |
| US-TP-002 | 验证用户登录状态 | Third-Party App | P0 | [auth/third-party-app](auth/third-party-app.md#故事-2验证用户登录状态-us-tp-002) |
| US-TP-003 | 检查用户权限 | Third-Party App | P0 | [auth/third-party-app](auth/third-party-app.md#故事-3检查用户权限-us-tp-003) |
| US-TP-004 | 获取用户信息 | Third-Party App | P0 | [auth/third-party-app](auth/third-party-app.md#故事-4获取用户信息-us-tp-004) |
| US-TP-005 | Client App 配置管理 | Third-Party App | P0 | [auth/third-party-app](auth/third-party-app.md#故事-5client-app-配置管理-us-tp-005) |
| US-TP-006 | 处理异常情况 | Third-Party App | P1 | [auth/third-party-app](auth/third-party-app.md#故事-6处理异常情况-us-tp-006) |
| US-TP-007 | 会话管理 | Third-Party App | P1 | [auth/third-party-app](auth/third-party-app.md#故事-7会话管理-us-tp-007) |
| US-TP-008 | 配置 Client App 跳转地址白名单 | Third-Party App | P0 | [auth/client-app-settings](auth/client-app-settings.md#故事-1配置-client-app-跳转地址白名单-us-tp-008) |
| US-TP-009 | 管理 Client App 图标 | Third-Party App | P0 | [auth/client-app-settings](auth/client-app-settings.md#故事-2管理-client-app-图标-us-tp-009) |
| US-TP-010 | 启用/禁用 Client App | Third-Party App | P0 | [auth/client-app-settings](auth/client-app-settings.md#故事-3启用禁用-client-app-us-tp-010) |
| US-TP-011 | 配置 Session 有效期策略 | Third-Party App | P0 | [auth/client-app-settings](auth/client-app-settings.md#故事-4配置-session-有效期策略-us-tp-011) |
| US-TO-001 | Realm 管理员启用/禁用 TOTP 功能 | TOTP User | P0 | [auth/totp](auth/totp.md#故事-1realm-管理员启用禁用-totp-功能-us-to-001) |
| US-TO-002 | 用户启用 TOTP 二次认证 | TOTP User | P0 | [auth/totp](auth/totp.md#故事-2用户启用-totp-二次认证-us-to-002) |
| US-TO-003 | 用户使用 TOTP 登录 | TOTP User | P0 | [auth/totp](auth/totp.md#故事-3用户使用-totp-登录-us-to-003) |
| US-TO-004 | 用户禁用 TOTP | TOTP User | P0 | [auth/totp](auth/totp.md#故事-4用户禁用-totp-us-to-004) |
| US-TO-005 | 用户重新生成 TOTP 密钥 | TOTP User | P1 | [auth/totp](auth/totp.md#故事-5用户重新生成-totp-密钥-us-to-005) |
| US-TO-006 | Realm 管理员强制启用 TOTP | TOTP User | P1 | [auth/totp](auth/totp.md#故事-6realm-管理员强制启用-totp-us-to-006) |
| US-TO-007 | 用户查看 TOTP 使用情况 | TOTP User | P2 | [auth/totp](auth/totp.md#故事-7用户查看-totp-使用情况-us-to-007) |
| US-PK-001 | Realm 管理员启用/禁用 Passkey 功能 | Realm Admin | P0 | [auth/passkey](auth/passkey.md#故事-1realm-管理员启用禁用-passkey-功能-us-pk-001) |
| US-PK-002 | Realm 管理员强制启用 Passkey | Realm Admin | P0 | [auth/passkey](auth/passkey.md#故事-2realm-管理员强制启用-passkey-us-pk-002) |
| US-PK-003 | Realm 管理员配置 Passkey 安全策略 | Realm Admin | P1 | [auth/passkey](auth/passkey.md#故事-3realm-管理员配置-passkey-安全策略-us-pk-003) |
| US-PK-004 | 用户注册 Passkey | Regular User | P0 | [auth/passkey](auth/passkey.md#故事-4用户注册-passkey-us-pk-004) |
| US-PK-005 | 用户使用 Passkey 直接登录 | Regular User | P0 | [auth/passkey](auth/passkey.md#故事-5用户使用-passkey-直接登录-us-pk-005) |
| US-PK-006 | 用户在密码登录后使用 Passkey 作为第二因素 | Regular User | P0 | [auth/passkey](auth/passkey.md#故事-6用户在密码登录后使用-passkey-作为第二因素-us-pk-006) |
| US-PK-007 | 用户查看和重命名已注册 Passkey | Regular User | P0 | [auth/passkey](auth/passkey.md#故事-7用户查看和重命名已注册-passkey-us-pk-007) |
| US-PK-008 | 用户在无法使用 Passkey 时回退到密码/TOTP | Regular User | P0 | [auth/passkey](auth/passkey.md#故事-8用户在无法使用-passkey-时回退到密码totp-us-pk-008) |
| US-PK-009 | 用户删除 Passkey | Regular User | P0 | [auth/passkey](auth/passkey.md#故事-9用户删除-passkey-us-pk-009) |
| US-PK-010 | Realm 管理员查看 Passkey 使用情况统计 | Realm Admin | P2 | [auth/passkey](auth/passkey.md#故事-10realm-管理员查看-passkey-使用情况统计-us-pk-010) |
| US-OE-001 | OAuth Provider 配置管理 | Realm Admin | P0 | [auth/oauth-extension](auth/oauth-extension.md#故事-1oauth-provider-配置管理-us-oe-001) |
| US-AL-001 | 在 iOS App 内使用 Apple 账号一键登录 | Regular User | P0 | [auth/support-mobile-apple-login](auth/support-mobile-apple-login.md#故事-1在-ios-app-内使用-apple-账号一键登录-us-al-001) |
| US-AL-002 | 接入方在 iOS App 中集成 Apple native 登录 | Third-Party App | P0 | [auth/support-mobile-apple-login](auth/support-mobile-apple-login.md#故事-2接入方在-ios-app-中集成-apple-native-登录-us-al-002) |
| US-AL-003 | Apple native 登录与已有账号关联 | Regular User | P1 | [auth/support-mobile-apple-login](auth/support-mobile-apple-login.md#故事-3apple-native-登录与已有账号关联-us-al-003) |
| US-EO-001 | 用户用邮箱验证码登录已有账号 | Regular User | P0 | [auth/email-otp-login](auth/email-otp-login.md#故事-1用户用邮箱验证码登录已有账号-us-eo-001) |
| US-EO-002 | 未注册邮箱验证成功后自动注册（受 Realm 注册政策门控） | Regular User | P0 | [auth/email-otp-login](auth/email-otp-login.md#故事-2未注册邮箱验证成功后自动注册-us-eo-002) |
| US-EO-003 | Realm 管理员配置邮箱验证码登录与自动注册 | Realm Admin | P0 | [auth/email-otp-login](auth/email-otp-login.md#故事-3realm-管理员配置邮箱验证码登录与自动注册-us-eo-003) |
| US-OT-001 | 通过 One Tap 在第三方应用一键登录 | Regular User | P0 | [auth/google-one-tap](auth/google-one-tap.md#故事-1通过-one-tap-在第三方应用一键登录-us-ot-001) |
| US-OT-002 | 第三方应用集成 One Tap | 第三方应用开发者 | P0 | [auth/google-one-tap](auth/google-one-tap.md#故事-2第三方应用集成-one-tap-us-ot-002) |
| US-OT-003 | One Tap 用户与已有账号关联 | Regular User | P1 | [auth/google-one-tap](auth/google-one-tap.md#故事-3one-tap-用户与已有账号关联-us-ot-003) |
| US-LD-001 | 用企业账号（LDAP）登录 | Regular User | P0 | [auth/support-ldap](auth/support-ldap.md#故事-1用企业账号ldap登录-us-ld-001) |
| US-LD-002 | 首次企业账号登录自动创建账号 | Regular User | P0 | [auth/support-ldap](auth/support-ldap.md#故事-2首次企业账号登录自动创建账号-us-ld-002) |
| US-LD-003 | Realm 管理员配置和管理本 Realm 的 LDAP 目录 | Realm Admin | P0 | [auth/support-ldap](auth/support-ldap.md#故事-3realm-管理员配置和管理本-realm-的-ldap-目录-us-ld-003) |
| US-LD-004 | 企业账号登录与其他登录及安全能力共存 | Regular User | P1 | [auth/support-ldap](auth/support-ldap.md#故事-4企业账号登录与其他登录及安全能力共存-us-ld-004) |
| US-BI-006 | 查看订阅列表 | Billing User | P0 | [billing/subscription](billing/subscription.md#故事-6查看订阅列表-us-bi-006) |
| US-BI-007 | 第三方应用查询套餐状态（SDK 集成） | Billing User | P0 | [billing/subscription](billing/subscription.md#故事-7第三方应用查询套餐状态sdk-集成-us-bi-007) |
| US-BI-008 | 查看订阅变更历史 | Billing User | P1 | [billing/subscription](billing/subscription.md#故事-8查看订阅变更历史-us-bi-008) |
| US-BI-009 | 查看自己的订阅变更历史 | Billing User | P1 | [billing/subscription](billing/subscription.md#故事-9查看自己的订阅变更历史-us-bi-009) |
| US-PO-001 | 配置积分套餐 | Points Admin | P0 | [billing/points-admin](billing/points-admin.md#故事-1配置积分套餐-us-po-001) |
| US-PO-002 | 查看所有用户积分账户 | Points Admin | P1 | [billing/points-admin](billing/points-admin.md#故事-2查看所有用户积分账户-us-po-002) |
| US-PO-003 | 查看用户积分交易历史 | Points Admin | P1 | [billing/points-admin](billing/points-admin.md#故事-3查看用户积分交易历史-us-po-003) |
| US-PO-004 | 管理积分套餐配置 | Points Admin | P2 | [billing/points-admin](billing/points-admin.md#故事-4管理积分套餐配置-us-po-004) |
| US-PO-005 | 查看套餐充值引导 | Points Admin | P2 | [billing/points-admin](billing/points-admin.md#故事-5查看套餐充值引导-us-po-005) |
| US-PO-006 | 配置 Realm 默认积分策略 | Points Admin | P0 | [billing/points-admin](billing/points-admin.md#故事-6配置-realm-默认积分策略-us-po-006) |
| US-PO-007 | 查看免费用户积分统计 | Points Admin | P1 | [billing/points-admin](billing/points-admin.md#故事-7查看免费用户积分统计-us-po-007) |
| US-PO-008 | 主动发放积分 | Points Admin | P0 | [billing/points-admin](billing/points-admin.md#故事-8主动发放积分-us-po-008) |
| US-PU-001 | 查看我的积分余额 | Points User | P0 | [billing/points-user](billing/points-user.md#故事-1查看我的积分余额-us-pu-001) |
| US-PU-002 | 查看我的交易历史 | Points User | P1 | [billing/points-user](billing/points-user.md#故事-2查看我的交易历史-us-pu-002) |
| US-PU-003 | 筛选交易记录 | Points User | P2 | [billing/points-user](billing/points-user.md#故事-3筛选交易记录-us-pu-003) |
| US-PU-004 | 查看分类积分余额 | Points User | P0 | [billing/points-user](billing/points-user.md#故事-15查看分类积分余额-us-pu-004) |
| US-PU-005 | 积分过期通知 | Points User | P1 | [billing/points-user](billing/points-user.md#故事-16积分过期通知-us-pu-005) |
| US-FU-001 | 注册时获得初始积分 | Free User | P0 | [billing/points-free-user](billing/points-free-user.md#故事-1注册时获得初始积分-us-fu-001) |
| US-FU-002 | 定期自动获得免费积分 | Free User | P0 | [billing/points-free-user](billing/points-free-user.md#故事-2定期自动获得免费积分-us-fu-002) |
| US-FU-003 | 升级到付费套餐时保留注册初始积分 | Free User | P1 | [billing/points-free-user](billing/points-free-user.md#故事-3升级到付费套餐时保留注册初始积分-us-fu-003) |
| US-FU-004 | 按时获得每期免费积分（不受分发延迟影响） | Free User | P0 | [billing/points-free-user](billing/points-free-user.md#故事-4按时获得每期免费积分不受分发延迟影响-us-fu-004) |
| US-PV-001 | 配置支付平台（Creem/Stripe） | Realm Admin | P0 | [billing/payment-provider](billing/payment-provider.md#故事-1配置支付平台-us-pv-001) |
| US-PV-002 | 查看支付平台配置 | Realm Admin | P0 | [billing/payment-provider](billing/payment-provider.md#故事-2查看支付平台配置-us-pv-002) |
| US-PV-003 | 编辑支付平台配置 | Realm Admin | P1 | [billing/payment-provider](billing/payment-provider.md#故事-3编辑支付平台配置-us-pv-003) |
| US-PV-004 | 删除支付平台配置 | Realm Admin | P1 | [billing/payment-provider](billing/payment-provider.md#故事-4删除支付平台配置-us-pv-004) |
| US-PV-005 | 查看支付平台使用统计 | Realm Admin | P2 | [billing/payment-provider](billing/payment-provider.md#故事-5查看支付平台使用统计-us-pv-005) |
| US-PU-006 | 购买积分包 | Regular User | P0 | [billing/points-package-purchase](billing/points-package-purchase.md#故事-1购买积分包-us-pu-006) |
| US-PU-007 | 查看积分包购买记录 | Regular User | P1 | [billing/points-package-purchase](billing/points-package-purchase.md#故事-2查看积分包购买记录-us-pu-007) |
| US-PU-008 | 积分包与订阅购买的区别 | Regular User | P1 | [billing/points-package-purchase](billing/points-package-purchase.md#故事-3理解积分包与订阅购买的区别-us-pu-008) |
| US-PU-009 | 按时使用本期积分（不受分发延迟影响） | Regular User | P0 | [billing/points-user](billing/points-user.md#故事-4按时使用本期积分不受分发延迟影响-us-pu-009) |
| US-PA-001 | 创建支付尝试（订阅或积分包） | System | P0 | [billing/payment-attempt](billing/payment-attempt.md#故事-1创建支付尝试订阅或积分包-us-pa-001) |
| US-PA-002 | 查询支付尝试状态 | System | P0 | [billing/payment-attempt](billing/payment-attempt.md#故事-2查询支付尝试状态-us-pa-002) |
| US-PA-003 | 处理支付成功后的履约 | System | P0 | [billing/payment-attempt](billing/payment-attempt.md#故事-3处理支付成功后的履约-us-pa-003) |
| US-PA-004 | 关闭过期的支付尝试 | System | P1 | [billing/payment-attempt](billing/payment-attempt.md#故事-4关闭过期的支付尝试-us-pa-004) |
| US-IV-001 | 创建发票 | Realm Admin | P0 | [billing/invoice](billing/invoice.md#故事-1创建发票-us-iv-001) |
| US-IV-002 | 编辑发票草稿 | Realm Admin | P0 | [billing/invoice](billing/invoice.md#故事-2编辑发票草稿-us-iv-002) |
| US-IV-003 | 查看发票列表 | Realm Admin | P0 | [billing/invoice](billing/invoice.md#故事-3查看发票列表-us-iv-003) |
| US-IV-004 | 查看发票详情 | Realm Admin | P0 | [billing/invoice](billing/invoice.md#故事-4查看发票详情-us-iv-004) |
| US-IV-005 | 开具发票 | Realm Admin | P0 | [billing/invoice](billing/invoice.md#故事-5开具发票-us-iv-005) |
| US-IV-006 | 作废发票 | Realm Admin | P1 | [billing/invoice](billing/invoice.md#故事-6作废发票-us-iv-006) |
| US-IV-007 | 标记发票已付 | Realm Admin | P0 | [billing/invoice](billing/invoice.md#故事-7标记发票已付-us-iv-007) |
| US-IV-008 | 查看我的发票 | Regular User | P1 | [billing/invoice](billing/invoice.md#故事-8查看我的发票-us-iv-008) |
| US-IV-009 | 系统标记逾期发票 | System | P1 | [billing/invoice](billing/invoice.md#故事-9系统标记逾期发票-us-iv-009) |
| US-IV-010 | 配置销售方信息 | Realm Admin | P0 | [billing/invoice](billing/invoice.md#故事-10配置销售方信息-us-iv-010) |
| US-IV-011 | 申请发票 | Regular User | P0 | [billing/invoice](billing/invoice.md#故事-11申请发票-us-iv-011) |
| US-IV-012 | 审核并开具用户申请的发票 | Realm Admin | P0 | [billing/invoice](billing/invoice.md#故事-12审核并开具用户申请的发票-us-iv-012) |
| US-IF-001 | 配置发票策略 | Realm Admin | P0 | [billing/invoice-fallback](billing/invoice-fallback.md#故事-1配置发票策略-us-if-001) |
| US-IF-002 | 系统同步 Stripe 发票 | System | P0 | [billing/invoice-fallback](billing/invoice-fallback.md#故事-2系统同步-stripe-发票-us-if-002) |
| US-IF-003 | 系统同步 Creem 交易税务数据 | System | P0 | [billing/invoice-fallback](billing/invoice-fallback.md#故事-3系统同步-creem-交易税务数据-us-if-003) |
| US-IF-004 | 查看外部 Provider 发票（管理员） | Realm Admin | P0 | [billing/invoice-fallback](billing/invoice-fallback.md#故事-4查看外部-provider-发票管理员-us-if-004) |
| US-IF-005 | 查看外部 Provider 发票（普通用户） | Regular User | P1 | [billing/invoice-fallback](billing/invoice-fallback.md#故事-5查看外部-provider-发票普通用户-us-if-005) |
| US-IF-006 | 下载外部发票 PDF 或查看 Provider 页面 | Realm Admin / Regular User | P1 | [billing/invoice-fallback](billing/invoice-fallback.md#故事-6下载外部发票-pdf-或查看-provider-页面-us-if-006) |
| US-EM-001 | 查看 Provider Entitlement 映射 | Realm Admin | P0 | [billing/entitlement-mapping](billing/entitlement-mapping.md#故事-1查看-provider-entitlement-映射-us-em-001) |
| US-EM-002 | 触发 Provider 产品同步 | Realm Admin | P1 | [billing/entitlement-mapping](billing/entitlement-mapping.md#故事-2触发-provider-产品同步-us-em-002) |
| US-EM-003 | Webhook 通过 Metadata 映射订阅 | System | P0 | [billing/entitlement-mapping](billing/entitlement-mapping.md#故事-3webhook-通过-metadata-映射订阅-us-em-003) |
| US-EM-004 | 基于 Entitlement 应用积分策略 | System | P0 | [billing/entitlement-mapping](billing/entitlement-mapping.md#故事-4基于-entitlement-应用积分策略-us-em-004) |
| US-EM-005 | SDK 通过 Entitlement 查询订阅状态 | Third-Party App | P0 | [billing/entitlement-mapping](billing/entitlement-mapping.md#故事-5sdk-通过-entitlement-查询订阅状态-us-em-005) |
| US-EM-006 | 查看订阅投影列表 | Realm Admin | P0 | [billing/entitlement-mapping](billing/entitlement-mapping.md#故事-6查看订阅投影列表-us-em-006) |
| US-BL-SYNC-001 | 同步携带 Stripe 商户自定义 metadata 并可查看 | Admin Realm | P1 | [billing/entitlement-mapping](billing/entitlement-mapping.md#故事-10同步时携带-stripe-商户自定义-metadata并在管理端可见-us-bl-sync-001) |
| US-BL-SYNC-002 | 列表展示产品名便于识别 | Admin Realm | P0 | [billing/entitlement-mapping](billing/entitlement-mapping.md#故事-11在-mapping-列表里看到产品名便于识别-us-bl-sync-002) |
| US-BL-SYNC-003 | Stripe/Creem 价格单位正确展示 | Admin Realm | P0 | [billing/entitlement-mapping](billing/entitlement-mapping.md#故事-12产品价格按-provider-单位正确展示不混淆-stripe-与-creem-us-bl-sync-003) |
| US-BL-SYNC-004 | 计费周期以 Stripe 为准、只读且不被人工覆盖 | Admin Realm | P0 | [billing/entitlement-mapping](billing/entitlement-mapping.md#故事-13计费周期以-stripe-为准只读且不被人工覆盖-us-bl-sync-004) |
| US-CB-001 | 管理积分账户目录 | Realm Admin | P0 | [billing/credit-bucket](billing/credit-bucket.md#故事-1管理积分账户目录-us-cb-001) |
| US-CB-002 | 为积分账户绑定 Client App 覆盖集 | Realm Admin | P0 | [billing/credit-bucket](billing/credit-bucket.md#故事-2为积分账户绑定-client-app-覆盖集-us-cb-002) |
| US-CB-003 | 将套餐/积分包归属到积分账户 | Realm Admin | P0 | [billing/credit-bucket](billing/credit-bucket.md#故事-3将套餐积分包归属到积分账户-us-cb-003) |
| US-CB-004 | 购买积分账户套餐/积分包 | Regular User | P0 | [billing/credit-bucket](billing/credit-bucket.md#故事-4购买积分账户套餐积分包-us-cb-004) |
| US-CB-005 | 查看按账户分组的积分余额 | Regular User | P0 | [billing/credit-bucket](billing/credit-bucket.md#故事-5查看按账户分组的积分余额-us-cb-005) |
| US-CB-006 | 查看积分账户维度的交易历史 | Regular User | P1 | [billing/credit-bucket](billing/credit-bucket.md#故事-6查看积分账户维度的交易历史-us-cb-006) |
| US-CB-007 | SDK 按 Client App 跨积分账户消费 | Third-Party App | P0 | [billing/credit-bucket](billing/credit-bucket.md#故事-7sdk-按-client-app-跨积分账户消费-us-cb-007) |
| US-CB-008 | 订阅生命周期按账户池发放与回收 | System | P0 | [billing/credit-bucket](billing/credit-bucket.md#故事-8订阅生命周期按账户池发放与回收-us-cb-008) |
| US-MWGR-001 | 为一个购买配置多条积分分发规则 | Realm Admin | P0 | [billing/multi-wallet-grant-rules](billing/multi-wallet-grant-rules.md#故事-1为一个购买配置多条积分分发规则-us-mwgr-001) |
| US-MWGR-002 | 为注册配置多条积分分发规则 | Realm Admin | P0 | [billing/multi-wallet-grant-rules](billing/multi-wallet-grant-rules.md#故事-2为注册配置多条积分分发规则-us-mwgr-002) |
| US-MWGR-003 | 一次业务事件完整执行多条积分规则 | System | P0 | [billing/multi-wallet-grant-rules](billing/multi-wallet-grant-rules.md#故事-3一次业务事件完整执行多条积分规则-us-mwgr-003) |
| US-MWGR-004 | 查看和停用积分分发规则 | Realm Admin | P1 | [billing/multi-wallet-grant-rules](billing/multi-wallet-grant-rules.md#故事-4查看和停用积分分发规则-us-mwgr-004) |
| US-PM-001 | 订阅续费记录每一次支付 | System | P0 | [billing/payment-invoice-mapping](billing/payment-invoice-mapping.md#故事-1订阅续费记录每一次支付-us-pm-001) |
| US-PM-002 | Creem 订阅续费同步发票 | System | P0 | [billing/payment-invoice-mapping](billing/payment-invoice-mapping.md#故事-2creem-订阅续费同步发票-us-pm-002) |
| US-PM-003 | 外部发票归属本地支付或订阅 | System | P1 | [billing/payment-invoice-mapping](billing/payment-invoice-mapping.md#故事-3外部发票归属本地支付或订阅-us-pm-003) |
| US-PM-004 | 发票-支付映射的补偿与可观测性 | System / Realm Admin | P2 | [billing/payment-invoice-mapping](billing/payment-invoice-mapping.md#故事-4发票-支付映射的补偿与可观测性-us-pm-004) |
| US-AU-001 | 查看 Realm 审计日志 | Realm Admin | P0 | [core/audit](core/audit.md#故事-1查看-realm-审计日志-us-au-001) |
| US-AU-002 | 按条件筛选审计日志 | Realm Admin | P0 | [core/audit](core/audit.md#故事-2按条件筛选审计日志-us-au-002) |
| US-AU-003 | 查看审计日志详情 | Realm Admin | P1 | [core/audit](core/audit.md#故事-3查看审计日志详情-us-au-003) |
| US-AU-004 | 查看 Admin Realm 审计日志 | Admin Realm | P0 | [core/audit](core/audit.md#故事-4查看-admin-realm-审计日志-us-au-004) |
| US-AU-005 | 系统自动记录核心操作 | System | P0 | [core/audit](core/audit.md#故事-5系统自动记录核心操作-us-au-005) |
| US-DC-001 | CLI 工具发起设备授权 | Third-Party App | P0 | [auth/device-code](auth/device-code.md#故事-1cli-工具发起设备授权-us-dc-001) |
| US-DC-002 | 用户在验证页面完成授权 | Regular User | P0 | [auth/device-code](auth/device-code.md#故事-2用户在验证页面完成授权-us-dc-002) |
| US-DC-003 | CLI 工具轮询获取令牌 | Third-Party App | P0 | [auth/device-code](auth/device-code.md#故事-3cli-工具轮询获取令牌-us-dc-003) |
| US-DC-004 | Realm Admin 配置 Device Code Grant | Realm Admin | P1 | [auth/device-code](auth/device-code.md#故事-4realm-admin-配置-device-code-grant-us-dc-004) |
| US-DC-005 | 设备验证页面 API | Third-Party App | P1 | [auth/device-code](auth/device-code.md#故事-5自定义设备码验证体验-us-dc-005) |
| US-TP-012 | 通过 SDK 管理 Realm | Third-Party App | P1 | [integration/sdk](integration/sdk.md#故事-1通过-sdk-管理-realm-us-tp-012) |
| US-TP-013 | 通过 SDK 管理用户 | Third-Party App | P0 | [integration/sdk](integration/sdk.md#故事-2通过-sdk-管理用户-us-tp-013) |
| US-TP-014 | 通过 SDK 管理 Client App | Third-Party App | P1 | [integration/sdk](integration/sdk.md#故事-3通过-sdk-管理-client-app-us-tp-014) |
| US-TP-015 | 第三方 Web SPA 发起 SSO 登录 | Third-Party App | P0 | [auth/third-party-app](auth/third-party-app.md#故事-8第三方-web-spa-发起-sso-登录-us-tp-015) |
| US-TP-016 | 第三方后端用授权码换取令牌 | Third-Party App | P0 | [auth/third-party-app](auth/third-party-app.md#故事-9第三方后端用授权码换取令牌-us-tp-016) |
| US-TP-017 | 通过 SDK 发放积分 | Third-Party App | P0 | [integration/sdk](integration/sdk.md#故事-4通过-sdk-发放积分-us-tp-017) |
| US-CUI-001 | 集成方前端完成注册与邮箱验证 | Third-Party App | P0 | [integration/custom-user-ui](integration/custom-user-ui.md#故事-1集成方前端完成注册与邮箱验证-us-cui-001) |
| US-CUI-002 | 集成方前端完成登录获得浏览器 token | Third-Party App | P0 | [integration/custom-user-ui](integration/custom-user-ui.md#故事-2集成方前端完成登录获得浏览器-token-us-cui-002) |
| US-CUI-003 | 集成方前端完成找回/重置密码 | Third-Party App | P0 | [integration/custom-user-ui](integration/custom-user-ui.md#故事-3集成方前端完成找回重置密码-us-cui-003) |
| US-CUI-004 | 集成方前端查看资料并修改昵称 | Third-Party App | P1 | [integration/custom-user-ui](integration/custom-user-ui.md#故事-4集成方前端查看资料并修改昵称-us-cui-004) |
| US-CUI-005 | 集成方前端完成高危安全操作 | Third-Party App | P0 | [integration/custom-user-ui](integration/custom-user-ui.md#故事-5集成方前端完成高危安全操作改密码--二因素--注销账号-us-cui-005) |
| US-CUI-006 | 集成方前端完成登出 | Third-Party App | P1 | [integration/custom-user-ui](integration/custom-user-ui.md#故事-6集成方前端完成登出-us-cui-006) |
| US-CUI-007 | 集成方前端完成积分与交易查看 | Third-Party App | P0 | [integration/custom-user-ui](integration/custom-user-ui.md#故事-7集成方前端完成积分与交易查看-us-cui-007) |
| US-CUI-008 | 集成方前端完成充值/购买 | Third-Party App | P0 | [integration/custom-user-ui](integration/custom-user-ui.md#故事-8集成方前端完成充值购买-us-cui-008) |
| US-CUI-009 | 集成方前端完成发票与订阅查看 | Third-Party App | P1 | [integration/custom-user-ui](integration/custom-user-ui.md#故事-9集成方前端完成发票与订阅查看-us-cui-009) |
| US-JS-001 | 初始化与跨域接入 | Third-Party App | P0 | [integration/js-sdk](integration/js-sdk.md#故事-1初始化与跨域接入-us-js-001) |
| US-JS-002 | 注册与邮箱验证 | Third-Party App | P0 | [integration/js-sdk](integration/js-sdk.md#故事-2注册与邮箱验证-us-js-002) |
| US-JS-003 | 找回与重置密码 | Third-Party App | P0 | [integration/js-sdk](integration/js-sdk.md#故事-3找回与重置密码-us-js-003) |
| US-JS-004 | 登录与多因素编排 | Third-Party App | P0 | [integration/js-sdk](integration/js-sdk.md#故事-4登录与多因素编排-us-js-004) |
| US-JS-005 | 自动静默刷新 | Third-Party App | P0 | [integration/js-sdk](integration/js-sdk.md#故事-5自动静默刷新-us-js-005) |
| US-JS-006 | 会话状态与登出 | Third-Party App | P1 | [integration/js-sdk](integration/js-sdk.md#故事-6会话状态与登出-us-js-006) |
| US-JS-007 | 可配置凭证存储 | Third-Party App | P1 | [integration/js-sdk](integration/js-sdk.md#故事-7可配置凭证存储-us-js-007) |
| US-JS-008 | 可区分的错误反馈 | Third-Party App | P1 | [integration/js-sdk](integration/js-sdk.md#故事-8可区分的错误反馈-us-js-008) |
| US-I18N-001 | 切换界面语言 | All Users | P0 | [core/i18n](core/i18n.md#故事-1切换界面语言-us-i18n-001) |
| US-I18N-002 | 查看翻译后的错误消息 | All Users | P0 | [core/i18n](core/i18n.md#故事-2查看翻译后的错误消息-us-i18n-002) |
| US-I18N-003 | 所有 UI 文本完成翻译 | All Users | P1 | [core/i18n](core/i18n.md#故事-3所有-ui-文本完成翻译-us-i18n-003) |
| US-WL-001 | 配置 Realm 品牌资产 | Realm Admin | P0 | [core/white-label](core/white-label.md#故事-1配置-realm-品牌资产-us-wl-001) |
| US-WL-002 | 终端用户看到品牌化 auth 流页面 | Regular User | P0 | [core/white-label](core/white-label.md#故事-2终端用户看到品牌化-auth-流页面-us-wl-002) |
| US-WL-003 | 主色对比度安全提示 | Realm Admin | P1 | [core/white-label](core/white-label.md#故事-3主色对比度安全提示-us-wl-003) |
| US-WL-004 | 资产 URL 引用与租户自备图床 | Realm Admin | P1 | [core/white-label](core/white-label.md#故事-4资产-url-引用与租户自备图床-us-wl-004) |
| US-CD-001 | 为本 Realm 配置自定义登录域名 | Realm Admin | P0 | [core/realm-custom-domain](core/realm-custom-domain.md#故事-1为本-realm-配置自定义登录域名-us-cd-001) |
| US-CD-003 | 自定义域名配置保存即生效 | Realm Admin | P1 | [core/realm-custom-domain](core/realm-custom-domain.md#故事-2自定义域名配置保存即生效-us-cd-003) |
| US-CD-005 | 未授权域名访问的拒绝 | Regular User | P1 | [core/realm-custom-domain](core/realm-custom-domain.md#故事-3未授权域名访问的拒绝-us-cd-005) |
| US-IAP-001 | 配置 IAP 支付渠道凭证 | Realm Admin | P0 | [billing/support-iap](billing/support-iap.md#故事-1配置-iap-支付渠道凭证-us-iap-001) |
| US-IAP-002 | 建立 IAP 商品与权益的映射 | Realm Admin | P0 | [billing/support-iap](billing/support-iap.md#故事-2建立-iap-商品与权益的映射-us-iap-002) |
| US-IAP-003 | 客户端提交凭证触发履约（主路径） | Third-Party App | P0 | [billing/support-iap](billing/support-iap.md#故事-3客户端提交凭证触发履约主路径-us-iap-003) |
| US-IAP-004 | Apple 服务端通知驱动生命周期与兜底 | System | P0 | [billing/support-iap](billing/support-iap.md#故事-4apple-服务端通知驱动生命周期与兜底-us-iap-004) |
| US-IAP-005 | 查询 IAP 订阅与权益状态 | Third-Party App | P1 | [billing/support-iap](billing/support-iap.md#故事-5查询-iap-订阅与权益状态-us-iap-005) |
| US-IAP-006 | 定时拉取对账（Google 生命周期主驱动 / Apple 补偿） | System | P0 | [billing/support-iap](billing/support-iap.md#故事-6定时拉取对账google-生命周期主驱动--apple-补偿us-iap-006) |
| US-WP-001 | 配置 WeChat Pay 凭据 | Realm Admin | P0 | [billing/wechat-support](billing/wechat-support.md#故事-1配置-wechat-pay-凭据-us-wp-001) |
| US-WP-002 | PC 扫码 Native 支付 | Regular User | P0 | [billing/wechat-support](billing/wechat-support.md#故事-2pc-扫码-native-支付-us-wp-002) |
| US-WP-003 | 微信内 JSAPI 唤起支付 | Regular User | P1 | [billing/wechat-support](billing/wechat-support.md#故事-3微信内-jsapi-唤起支付-us-wp-003) |
| US-WP-004 | WeChat 回调验签、解密与幂等履约 | System | P0 | [billing/wechat-support](billing/wechat-support.md#故事-4wechat-回调验签解密与幂等履约-us-wp-004) |
| US-WP-005 | 平台证书自动获取与刷新 | System | P0 | [billing/wechat-support](billing/wechat-support.md#故事-5平台证书自动获取与刷新-us-wp-005) |
| US-BM-001 | 配置买断商品映射 | Realm Admin | P0 | [billing/pay_model](billing/pay_model.md#故事-1配置买断商品映射-us-bm-001) |
| US-BM-002 | 购买卖断商品 | Regular User | P0 | [billing/pay_model](billing/pay_model.md#故事-4购买买断商品-us-bm-002) |
| US-BM-003 | 恢复买断购买 | Regular User | P0 | [billing/pay_model](billing/pay_model.md#故事-5恢复买断购买-us-bm-003) |
| US-BM-004 | 查询买断权益（与 US-NR-004 同场景） | Third-Party App | P1 | [billing/pay_model](billing/pay_model.md#故事-7查询权益非续期订阅与买断us-nr-004买断部分对应-us-bm-004) |
| US-BM-005 | 退款或撤销买断（回收支付来源角色） | System | P0 | [billing/pay_model](billing/pay_model.md#故事-8退款或撤销买断-us-bm-005) |
| US-NR-001 | 配置非续期订阅映射 | Realm Admin | P0 | [billing/pay_model](billing/pay_model.md#故事-2配置非续期订阅映射-us-nr-001) |
| US-NR-002 | 管理非续期订阅（区分计费类型与截止时间） | Realm Admin | P1 | [billing/pay_model](billing/pay_model.md#故事-3管理非续期订阅-us-nr-002) |
| US-NR-003 | 购买非续期订阅 | Regular User | P0 | [billing/pay_model](billing/pay_model.md#故事-6购买非续期订阅-us-nr-003) |
| US-NR-004 | 查询非续期订阅（与 US-BM-004 同场景） | Third-Party App | P1 | [billing/pay_model](billing/pay_model.md#故事-7查询权益非续期订阅与买断us-nr-004买断部分对应-us-bm-004) |
| US-NR-005 | 处理非续期订阅生命周期 | System | P0 | [billing/pay_model](billing/pay_model.md#故事-9处理非续期订阅生命周期-us-nr-005) |
| US-PW-001 | 配置 entitlement 映射的 role 授予维度 | Realm Admin | P0 | [billing/support-paywall](billing/support-paywall.md#故事-1配置-entitlement-映射的-role-授予维度-us-pw-001) |
| US-PW-002 | 一次性纯权益购买成功且不报错 | Realm Admin | P1 | [billing/support-paywall](billing/support-paywall.md#故事-2一次性纯权益购买成功且不报错-us-pw-002) |
| US-PW-003 | 支付成功自动授予 role | System | P0 | [billing/support-paywall](billing/support-paywall.md#故事-3支付成功自动授予-role-us-pw-003) |
| US-PW-004 | 一次性永久权益一人一次防重复购买 | Regular User | P0 | [billing/support-paywall](billing/support-paywall.md#故事-4一次性永久权益一人一次防重复购买-us-pw-004) |
| US-PW-005 | 支付事件触发 role 撤销 | System | P0 | [billing/support-paywall](billing/support-paywall.md#故事-5支付事件触发-role-撤销-us-pw-005) |
| US-PW-006 | 第三方应用凭 role 一行判断解锁功能 | Third-Party App | P1 | [billing/support-paywall](billing/support-paywall.md#故事-6第三方应用凭-role-一行判断解锁功能-us-pw-006) |
| US-MC-003 | 购买页按货币分组、显式选择货币（无默认） | Regular User | P0 | [billing/multiple-currency](billing/multiple-currency.md#故事-3购买页按货币分组显式选择货币无默认-us-mc-003) |
| US-MC-004 | 按（显式选定的）货币价格行发起购买 | Regular User | P0 | [billing/multiple-currency](billing/multiple-currency.md#故事-4按显式选定的货币价格行发起购买-us-mc-004) |
| US-MC-005 | 查询可购权益支持的货币集合 | Third-Party App | P0 | [billing/multiple-currency](billing/multiple-currency.md#故事-5查询可购权益支持的货币集合-us-mc-005) |
| US-MC-006 | Creem / IAP / WeChat Pay 单一价格降级展示 | Regular User | P2 | [billing/multiple-currency](billing/multiple-currency.md#故事-6creem--iap--wechat-pay-单一价格降级展示-us-mc-006) |

---

## 按领域分类

### Core 核心功能

| 角色 | 文档 | 相关 PRD |
|------|------|---------|
| Admin Realm | [core/admin-realm.md](core/admin-realm.md) | [Realm PRD](/docs/prd/core/realm.md) |
| 自助开通 Realm | [core/realm-create.md](core/realm-create.md) | [SaaS 自助注册开通 Realm PRD](/docs/prd/core/realm-create.md) |
| Realm Admin | [core/realm-admin.md](core/realm-admin.md), [core/builtin-protection.md](core/builtin-protection.md), [core/legal-consent-account-deletion.md](core/legal-consent-account-deletion.md) | [Users PRD](/docs/prd/core/users.md), [Permissions PRD](/docs/prd/auth/permissions.md), [Client Apps PRD](/docs/prd/integration/client-app.md), [Realm Settings PRD](/docs/prd/core/realm-settings.md), [Dashboard PRD](/docs/prd/core/dashboard.md), [API Key Roles PRD](/docs/prd/integration/api-key-roles.md), [Legal Consent PRD](/docs/prd/core/legal-consent-account-deletion.md) |
| Regular User | [core/regular-user.md](core/regular-user.md), [core/legal-consent-account-deletion.md](core/legal-consent-account-deletion.md) | [Users PRD](/docs/prd/core/users.md), [OAuth PRD](/docs/prd/auth/oauth.md), [Legal Consent PRD](/docs/prd/core/legal-consent-account-deletion.md) |
| i18n | [core/i18n.md](core/i18n.md) | [i18n PRD](/docs/prd/core/i18n.md) |
| Audit | [core/audit.md](core/audit.md) | [Audit PRD](/docs/prd/core/audit.md) |
| White-label | [core/white-label.md](core/white-label.md) | [White-label PRD](/docs/prd/core/ui-custom.md) |
| Custom Domain | [core/realm-custom-domain.md](core/realm-custom-domain.md) | [Realm 自定义域名 PRD](/docs/prd/core/realm-custom-domain.md) |

### Auth 认证与授权

| 角色 | 文档 | 相关 PRD |
|------|------|---------|
| Third-Party App | [auth/third-party-app.md](auth/third-party-app.md), [auth/client-app-settings.md](auth/client-app-settings.md) | [OAuth PRD](/docs/prd/auth/oauth.md), [Client Apps PRD](/docs/prd/integration/client-app.md) |
| TOTP User | [auth/totp.md](auth/totp.md) | [TOTP PRD](/docs/prd/auth/totp.md) |
| Passkey User | [auth/passkey.md](auth/passkey.md) | [Passkey PRD](/docs/prd/auth/passkey.md) |
| OAuth Extension | [auth/oauth-extension.md](auth/oauth-extension.md) | [OAuth PRD](/docs/prd/auth/oauth.md) |
| Google One Tap | [auth/google-one-tap.md](auth/google-one-tap.md) | [Google One Tap PRD](/docs/prd/auth/google-one-tap.md) |
| WeChat OAuth | [auth/wechat-oauth.md](auth/wechat-oauth.md) | [WeChat OAuth PRD](/docs/prd/auth/wechat-oauth.md) |
| Apple Native Login | [auth/support-mobile-apple-login.md](auth/support-mobile-apple-login.md) | [Apple native 登录 PRD](/docs/prd/auth/support-mobile-apple-login.md) |
| LDAP 企业目录登录 | [auth/support-ldap.md](auth/support-ldap.md) | [LDAP 企业目录登录 PRD](/docs/prd/auth/support-ldap.md) |
| Device Code | [auth/device-code.md](auth/device-code.md) | [Device Code PRD](/docs/prd/auth/device-code.md) |

### Billing 计费相关

| 角色 | 文档 | 相关 PRD |
|------|------|---------|
| Billing User | [billing/subscription.md](billing/subscription.md) | [Subscription PRD](/docs/prd/billing/subscription.md) |
| Points Admin | [billing/points-admin.md](billing/points-admin.md) | [Points PRD](/docs/prd/billing/points.md) |
| Points User | [billing/points-user.md](billing/points-user.md), [billing/points-free-user.md](billing/points-free-user.md) | [Points PRD](/docs/prd/billing/points.md), [Subscription PRD](/docs/prd/billing/subscription.md) |
| Points Package Purchase | [billing/points-package-purchase.md](billing/points-package-purchase.md) | [Subscription PRD](/docs/prd/billing/subscription.md) |
| Payment Provider | [billing/payment-provider.md](billing/payment-provider.md) | [Subscription PRD](/docs/prd/billing/subscription.md) |
| Payment Attempt | [billing/payment-attempt.md](billing/payment-attempt.md) | [Subscription PRD](/docs/prd/billing/subscription.md) |
| Invoice | [billing/invoice.md](billing/invoice.md) | [Invoice PRD](/docs/prd/billing/invoice.md) |
| Invoice Fallback | [billing/invoice-fallback.md](billing/invoice-fallback.md) | [Invoice PRD](/docs/prd/billing/invoice.md) |
| Payment Invoice Mapping | [billing/payment-invoice-mapping.md](billing/payment-invoice-mapping.md) | [Invoice PRD](/docs/prd/billing/invoice.md) |
| Entitlement Mapping | [billing/entitlement-mapping.md](billing/entitlement-mapping.md) | [Subscription PRD](/docs/prd/billing/subscription.md) |
| IAP | [billing/support-iap.md](billing/support-iap.md) | [IAP PRD](/docs/prd/billing/support-iap.md) |
| WeChat Pay | [billing/wechat-support.md](billing/wechat-support.md) | [WeChat Pay PRD](/docs/prd/billing/wechat-support.md) |
| 多货币 | [billing/multiple-currency.md](billing/multiple-currency.md) | [多货币 PRD](/docs/prd/billing/multiple-currency.md) |
| 积分账户 | [billing/credit-bucket.md](billing/credit-bucket.md) | [Points PRD](/docs/prd/billing/points.md) |

### Integration 集成

| 角色 | 文档 | 相关 PRD |
|------|------|---------|
| SDK | [integration/sdk.md](integration/sdk.md) | [SDK PRD](/docs/prd/integration/sdk.md) |
| JS 浏览器 SDK | [integration/js-sdk.md](integration/js-sdk.md) | [JS 浏览器 SDK PRD](/docs/prd/integration/js-sdk.md) |
| 自建用户 UI | [integration/custom-user-ui.md](integration/custom-user-ui.md) | [自建用户 UI PRD](/docs/prd/integration/custom-user-ui.md) |

## 特殊文档

| 文档 | 说明 |
|------|------|
| [_README.md](_README.md) | 用户故事编写指南 |
| [_roles.md](_roles.md) | 角色定义文档 |

## 相关文档

- **PRD 文档索引**: [docs/prd/index.md](/docs/prd/index.md)
- **角色定义**: [_roles.md](_roles.md)
