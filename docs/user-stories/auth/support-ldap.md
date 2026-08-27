# LDAP 登录 用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：用企业账号（LDAP）登录 [US-LD-001]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在登录页选择"企业账号登录"，输入我的企业目录用户名和密码完成登录
**从而**：不需要在 Herald 另设一套密码，直接用公司已有账号访问接入 Herald 的应用

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：已关联用户登录成功**
```gherkin
Given 该 Realm 已配置并启用 LDAP 目录
And 用户的企业身份已关联到该 Realm 的 Herald 账号
When 用户在登录页选择"企业账号登录"，输入正确的企业用户名和密码
Then 用户通过企业目录认证，登录成功并进入后续流程（如需二因素则先完成二因素验证）
```

**场景 2：密码错误（防枚举）**
```gherkin
Given 该 Realm 已配置并启用 LDAP 目录
When 用户输入的企业用户名在目录中不存在，或密码错误
Then 登录失败，提示与普通密码登录一致的泛化错误（如"账号或密码错误"），不区分"目录里没有这个人"与"密码错误"
```

**场景 3：目录服务不可达或超时**
```gherkin
Given 该 Realm 已启用的 LDAP 目录服务器当前不可达或响应超时
When 用户提交企业账号登录
Then 登录失败，提示"登录暂时不可用，请稍后再试"
And 提示不暴露目录服务器地址等内部配置
And 登录页其他登录方式不受影响，仍可正常使用
```

**场景 4：账号被禁用**
```gherkin
Given 用户关联的 Herald 账号已被禁用
When 用户输入正确的企业账号密码并通过目录认证
Then 登录被拒绝，提示账号已被禁用
```

**场景 5：Realm 未启用 LDAP**
```gherkin
Given 该 Realm 未配置或未启用 LDAP 目录
When 用户访问该 Realm 的登录页
Then 登录页不展示"企业账号登录"入口
And 直接向该 Realm 提交企业账号登录请求被拒绝，不创建任何账号或会话
```

---

### 故事 2：首次企业账号登录自动创建账号 [US-LD-002]

**优先级**: P0

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：首次用企业账号登录时，系统自动为我创建 Herald 账号并完成登录
**从而**：不需要管理员逐个预先开户，一次登录同时完成开户和进入

**【验收标准】**

**场景 1：首次登录自动建号（JIT）**
```gherkin
Given 该 Realm 已配置并启用 LDAP 目录
And 用户的企业目录身份尚未关联任何 Herald 账号
When 用户输入正确的企业用户名和密码，通过目录认证
Then 系统自动创建 Herald 账号（该账号没有本地密码）并完成登录
And 后续该用户再次用同一企业身份登录时，命中同一账号，不重复建号
```

**场景 2：目录邮箱与已有账号一致时关联而非重复建号**
```gherkin
Given 用户此前已在该 Realm 用邮箱注册了 Herald 账号
And 目录认证后取得的邮箱与该账号邮箱一致
When 用户首次用企业账号登录
Then 系统将企业身份关联到已有账号并登录成功
And 不创建重复账号
```

**场景 3：目录未返回邮箱时仍可登录**
```gherkin
Given 用户首次用企业账号登录
And 目录中该用户没有邮箱属性
When 用户通过目录认证
Then 系统仍创建账号并登录成功（邮箱记为占位且未验证）
And 不因邮箱缺失拒绝登录
```

**场景 4：Realm 关闭公开注册时首次登录仍可建号**
```gherkin
Given 该 Realm 已启用 LDAP 目录
And 该 Realm 的公开注册开关处于关闭状态
When 企业用户首次用企业账号登录并通过目录认证
Then 系统仍自动创建账号并登录成功
（因为管理员启用 LDAP 目录已表达了对该目录供给的授权，企业目录登录不是公开自注册）
```

**场景 5：首登建号前完成协议同意（如 Realm 要求）**
```gherkin
Given 该 Realm 配置了生效的用户协议/隐私政策（登录即同意模式）
When 用户首次用企业账号登录并通过目录认证
Then 用户须先表达对当前生效协议版本的同意，系统才创建账号并完成登录
And 同意记录与协议版本绑定，可被审计
```

---

### 故事 3：Realm 管理员配置和管理本 Realm 的 LDAP 目录 [US-LD-003]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：为本 Realm 配置企业 LDAP 目录（服务器地址、Base DN、服务账号、用户搜索规则、属性映射）并启用/停用
**从而**：本企业员工用目录账号即可登录接入本 Realm 的应用，无需逐个开户

**【验收标准】**

**场景 1：配置并启用后登录入口出现**
```gherkin
Given Realm Admin 已进入本 Realm 的设置管理界面
When 管理员完整填写 LDAP 目录配置并启用
Then 配置保存成功，仅对本 Realm 生效
And 该 Realm 登录页出现"企业账号登录"入口
```

**场景 2：停用后平滑降级**
```gherkin
Given 该 Realm 已启用 LDAP 目录且已有员工通过企业账号登录过
When 管理员停用 LDAP 目录
Then 该 Realm 登录页不再展示"企业账号登录"入口，企业账号登录请求被拒绝
And 已创建的员工账号不丢失，仍可使用其已绑定的其他登录方式（如已设置的密码、Passkey）
```

**场景 3：仅能管理本 Realm 配置**
```gherkin
Given Realm Admin 属于 Realm A
When 该管理员尝试查看或修改 Realm B 的 LDAP 目录配置
Then 访问被拒绝
```

**场景 4：服务账号密码不外泄**
```gherkin
Given 管理员已配置 LDAP 服务账号（bind 密码）
When 管理员再次查看配置，或任何未授权方查询本 Realm 配置
Then 服务账号密码不以明文展示，不出现在公共配置和日志中
```

**场景 5：不安全的明文目录地址被拒绝**
```gherkin
Given 管理员填写的目录服务器地址为不加密的明文连接（未启用加密信道）
When 管理员保存配置
Then 系统拒绝保存并提示必须使用加密连接（如 ldaps 或 StartTLS）
（企业账号密码只允许在加密信道中传输）
```

**场景 6：LDAP 登录事件可审计**
```gherkin
Given 该 Realm 已启用 LDAP 目录
When 用户通过企业账号登录（无论成功或失败）
Then 管理员可在审计日志中查到该次登录事件，认证方式标记为企业目录（LDAP）
```

---

### 故事 4：企业账号登录与其他登录及安全能力共存 [US-LD-004]

**优先级**: P1

**【用户故事】**
**作为**：普通用户（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：用企业账号登录时，与其他登录方式和账户安全能力（二因素验证、第三方应用授权）保持一致体验
**从而**：企业账号登录不是"二等入口"，账户安全等级不因登录方式降低

**【验收标准】**

**场景 1：已绑定 TOTP 的用户被要求二因素验证**
```gherkin
Given 用户账号已绑定 TOTP
When 用户通过企业账号登录并通过目录认证
Then 系统要求用户完成 TOTP 验证后才允许登录完成
```

**场景 2：从第三方应用发起登录时正常完成授权**
```gherkin
Given 用户从接入 Herald 的第三方应用发起登录（授权码流程）
When 用户在该次登录中选择企业账号登录并认证成功
Then 登录流程按既有授权链路继续，第三方应用正常获得授权并完成登录
```

**场景 3：LDAP 建号账号不适用本地密码登录**
```gherkin
Given 用户账号由企业账号首次登录自动创建，从未设置过本地密码
When 用户尝试在密码登录表单中用该账号邮箱 + 任意密码登录
Then 登录失败，得到与密码错误一致的泛化提示，不暴露账号细节
And 用户仍可通过"企业账号登录"入口正常登录
```

---

## 备注

### 业务规则

1. LDAP 目录配置按 Realm 隔离；每 Realm 一份目录配置（DEC-support-ldap-001/005）。
2. 用户匹配策略：按目录身份（DN）→ 邮箱 → 创建，与既有第三方登录匹配策略同构；目录邮箱由企业管理员维护，视为可信来源（DEC-support-ldap-008）。
3. 首次登录自动建号不受 Realm 公开注册开关限制：管理员启用 LDAP 目录即视为对该目录供给的授权（DEC-support-ldap-007）。
4. 企业账号登录完整继承现有登录安全管线：人机验证（按 Client App 配置）、IP+标识符限流、二因素验证、协议同意、审计（DEC-support-ldap-006）。
5. 不做 LDAP 组→角色映射、不做后台目录同步、不做 Windows 桌面单点登录（DEC-support-ldap-001/003）。

### 与现有用户故事的关系

- 扩展 [US-RU-002/003](/docs/user-stories/core/regular-user.md) 的登录与第三方登录语义，新增企业目录作为第一因子来源
- 与 [US-TP-001](/docs/user-stories/auth/third-party-app.md) 授权码登录兼容，企业账号登录可嵌入下游授权流程
- 协议同意复用 [docs/user-stories/core/legal-consent-account-deletion.md](/docs/user-stories/core/legal-consent-account-deletion.md) 的"登录即同意"模型
- 管理端配置形态参照 [US-EO-003](/docs/user-stories/auth/email-otp-login.md)（Realm 管理员配置替代登录方式）
- 二因素验证复用 [docs/user-stories/auth/totp.md](/docs/user-stories/auth/totp.md) 既有能力

---

## 相关文档

- **PRD**: [docs/prd/auth/support-ldap.md](/docs/prd/auth/support-ldap.md)
- **角色定义**: [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)
