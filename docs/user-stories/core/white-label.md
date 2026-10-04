# White-label（登录/注册 UI 定制）用户故事

> 角色定义见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)

## 用户故事

### 故事 1：配置 Realm 品牌资产 [US-WL-001]

**优先级**: P0

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在管理后台配置本 Realm 的品牌资产（logo、主色、背景、页脚文案、登录/注册页标题与副标题文案）
**从而**：让本 Realm 终端用户在登录/注册及其他 auth 流页面看到属于本租户品牌的页面，而非默认的 Herald 品牌呈现

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：成功配置全部品牌资产**
```gherkin
Given 我是 realm-1 的 Realm Admin
And 我已进入管理后台 Settings 页面的品牌化配置入口
When 我填入 logo 图片 URL、主色（accent color）、背景（图片 URL 或渐变）、页脚文案、登录/注册页标题与副标题文案
And 我保存配置
Then 配置保存成功并显示成功反馈
And 配置仅作用于 realm-1，不影响其他 Realm
```

**场景 2：未配置字段回退默认**
```gherkin
Given realm-1 的 Realm Admin 未填写某项品牌资产（例如未填 logo URL）
When 终端用户访问 realm-1 的 auth 流页面
Then 该未填字段回退到默认 Herald 呈现（如无 logo 时显示默认 "Herald" 文字）
And 其余已配置字段仍按 realm-1 配置呈现
```

**场景 3：仅 Realm Admin 可配置**
```gherkin
Given 我是 realm-1 的普通用户（Regular User）
When 我尝试访问管理后台的品牌化配置入口
Then 系统拒绝访问
And 我看不到该配置入口
```

**场景 4：草稿不生效、发布后生效、可恢复上一版**
```gherkin
Given 我是 realm-1 的 Realm Admin
And realm-1 已有一版已发布的品牌配置
When 我把新品牌配置保存为草稿
Then 终端用户访问 realm-1 的 auth 流页面时看到的仍是已发布版本
When 我发布草稿
Then 终端用户随即看到新版品牌呈现
When 我执行"恢复上一版"
Then 品牌呈现回退到发布前的上一版配置
```

---

### 故事 2：终端用户看到品牌化 auth 流页面 [US-WL-002]

**优先级**: P0

**【用户故事】**
**作为**：Regular User（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在不同 Realm 的登录、注册及其他 auth 流页面看到该 Realm 配置的品牌呈现（logo、主色、背景、页脚、标题/副标题文案）
**从而**：页面表现为该租户自己的品牌，建立品牌识别与信任

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：登录页呈现 Realm 品牌**
```gherkin
Given realm-1 已配置 logo、主色、背景、页脚、标题/副标题文案
When 终端用户访问 realm-1 的登录页
Then 页面头部显示 realm-1 配置的 logo
And 主按钮、链接等品牌色使用 realm-1 配置的主色
And 页面背景使用 realm-1 配置的背景
And 页面页脚显示 realm-1 配置的页脚文案
And 页面标题/副标题使用 realm-1 配置的文案
```

**场景 2：品牌化覆盖所有 auth 流页面与子状态**
```gherkin
Given realm-1 已配置品牌资产
When 终端用户在 realm-1 的任意 auth 流页面（登录、注册、忘记密码、邮箱验证、OAuth 同意、TOTP、passkey 等）及其子状态间流转
Then 所有这些页面均呈现 realm-1 配置的品牌资产
And 在任一 auth 流子状态下品牌资产不会丢失或回退为默认
```

**场景 3：logo 加载失败回退**
```gherkin
Given realm-1 配置的 logo URL 无法加载（如图片被删除或链接失效）
When 终端用户访问 realm-1 的 auth 流页面
Then logo 区域不显示破损图标
And 回退显示默认 "Herald" 文字
And 其余品牌资产仍正常呈现
```

---

### 故事 3：主色对比度安全提示 [US-WL-003]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：在配置主色时，若该色导致按钮文字对比度低于 WCAG AA 标准，系统提示我但不阻止保存
**从而**：我意识到可读性风险，但保留品牌色决策的最终控制权

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：对比度达标时不提示**
```gherkin
Given 我在配置 realm-1 的主色
When 我填入一个与按钮文字对比度达到 WCAG AA 标准（≥4.5:1）的颜色
Then 系统不显示对比度警告
And 我可以保存配置
```

**场景 2：对比度不达标时仅警告不拦截**
```gherkin
Given 我在配置 realm-1 的主色
When 我填入一个与按钮文字对比度低于 WCAG AA 标准（<4.5:1）的颜色
Then 系统显示对比度不足的警告文案
And 我仍可以保存该配置（不被拦截）
And 保存成功后该主色按配置应用于终端用户页面
```

---

### 故事 4：资产 URL 引用与租户自备图床 [US-WL-004]

**优先级**: P1

**【用户故事】**
**作为**：Realm Admin（详见 [docs/user-stories/_roles.md](/docs/user-stories/_roles.md)）
**我希望**：logo 与背景通过图片 URL 引用方式配置
**从而**：无需依赖 Herald 提供上传存储即可完成品牌化

**【验收标准】**

> 验收标准只描述用户动作与可见结果，不写 API 路径、数据表、字段变更、技术实现步骤。

**场景 1：以 URL 引用配置 logo 与背景**
```gherkin
Given 我在配置 realm-1 的品牌资产
When 我填入外部可访问的 logo 图片 URL 与背景图片 URL（或背景渐变描述）
And 我保存配置
Then 终端用户访问 realm-1 auth 流页面时通过该 URL 加载图片
And 我无需在 Herald 上传图片文件
```

**场景 2：URL 失效时的可见回退**
```gherkin
Given realm-1 配置的背景 URL 无法加载
When 终端用户访问 realm-1 的 auth 流页面
Then 背景回退为默认呈现（不显示破损样式）
And 页面其他品牌资产不受影响
```
