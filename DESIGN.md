# Herald 前端设计规范（DESIGN.md）

> **本文件是 Herald 前端视觉设计的唯一事实来源（single source of truth）。**
> 所有前端工作——新页面、改组件、调样式——必须先对照本规范；规范要改，先改这里再改代码。
>
> 设计蓝本：[impeccable.style](https://impeccable.style/) 的「kinpaku（金箔）」设计体系。
> 其设计令牌直接取自该站公开的 `kinpaku-tokens.css`，并映射到本项目 shadcn/Tailwind v4 的变量结构（`frontend/src/styles.css`）。
>
> 视觉预览：`design-preview/herald-kinpaku-preview.svg`（浏览器直接打开）。

---

## 1. 设计原则

来自 impeccable.style 的设计哲学，按 Herald（多租户认证控制台）的场景改写：

1. **克制（Calm）**。全局只有五组颜色：纸、墨、金、铜绿、朱。不加第六组。
2. **一个赢家（One winner）**。每个视图只有一个视觉焦点；每个屏幕只有一个实心主按钮。标题和按钮不抢注意力——按钮赢。
3. **层级靠字号、字重、灰度，不靠颜色堆砌。**
4. **金箔法则**：金是「箔」不是「漆」。金只用于细线、指示条、微光、深色面板上的高亮文字；**永远不做纸面上的文字色，永远不做大面积底色**。按钮底色永远是墨。
5. **可见标签**。表单 label 永远可见；placeholder 只作示例，不承担标签职责。
6. **对比度**。正文 ≥ 4.5:1；大字/图形 ≥ 3:1。金色（`#FFBA00`）在纸上只有 1.4:1，禁止作纸面文字。
7. **统一圆角**。8px 体系：卡片/按钮/输入框/对话框 8px，微型芯片/代码 3px，胶囊 999px。不混用其他值。
8. **动效有目的**。120ms 即时反馈，200ms 面板归位，一条缓动曲线。默认无弹跳、无脉冲。
9. **仪器感（Instrument）**。控制台要像精密仪器：紧凑密度、等宽字读数、深色仪器面板、LED 式微光。
10. **蒸馏（Distill）**。一个界面做一件事。删到删不动为止。空状态是展示克制的地方：一句眉题、一句话、一个主操作。

### 禁止事项（反 AI-slop 清单）

- ❌ 渐变按钮、渐变横幅、彩色投影（金箔 LED 微光是唯一例外，且仅限深色面板）
- ❌ 斜体衬线标题、脉冲点（pulsing dots）、emoji 当图标、彩虹色板
- ❌ 金色文字出现在纸面上；白字放在金色上
- ❌ 同一视图两个实心主按钮
- ❌ 只有 placeholder 没有 label 的表单
- ❌ 动画超过 250ms；装饰性无限循环动画（spinner 除外）
- ❌ 引入本规范之外的圆角、阴影、颜色值

---

## 2. 色彩（Color）

全部使用 OKLCH。hex 仅为近似参考，**实现以 oklch 为准**。

### 2.1 纸与墨（基础面，中性无色相）

| 令牌 | 值 | hex ≈ | 用途 |
|---|---|---|---|
| paper | `oklch(97.8% 0 0)` | `#F8F8F8` | 页面背景 |
| paper-raised | `oklch(99.5% 0 0)` | `#FDFDFD` | 卡片、弹层 |
| paper-deep | `oklch(95% 0 0)` | `#EEEEEE` | 次级底、内嵌面、代码块底 |
| gray | `oklch(92% 0 0)` | `#E4E4E4` | hover 背景、分割区块 |
| ink | `oklch(13% 0 0)` | `#070707` | 标题、主按钮底、最强文字 |
| text | `oklch(22% 0 0)` | `#1B1B1B` | 正文 |
| text-muted | `oklch(46% 0 0)` | `#585858` | 次要文字、副标题 |
| text-faint | `oklch(51% 0 0)` | `#666666` | 占位符、坐标轴、辅助说明 |
| rule | `oklch(13% 0 0 / 0.08)` | — | 发丝分割线、卡片描边 |
| input-line | `oklch(13% 0 0 / 0.12)` | — | 输入框描边（比 rule 略实） |

### 2.2 金（主强调，kinpaku）

| 令牌 | 值 | hex ≈ | 用途 |
|---|---|---|---|
| kinpaku | `oklch(84% .19 80.46)` | `#FFBA00` | 侧栏活动指示、深色面板高亮、LED 微光 |
| kinpaku-rich | `oklch(77% .13 82)` | `#DDAB46` | 图表主系列、纸面上的金色图形（细线需加粗或加深） |
| kinpaku-deep | `oklch(61% .085 78)` | `#9F7D45` | 金色系可读文字（头像文字、装饰细节） |
| on-gold | `oklch(14% .018 95)` | `#0B0903` | 金底之上的文字（对比 9.3:1） |

### 2.3 铜绿（辅助/状态，patina）

| 令牌 | 值 | hex ≈ | 用途 |
|---|---|---|---|
| patina | `oklch(70% .12 188)` | `#0FB6AC` | info 底色、图表第二系列 |
| patina-deep | `oklch(45% .1 190)` | `#006662` | **聚焦环**、成功状态、图表系列 |
| patina-ink | `oklch(20% .04 190)` | ≈`#023836` | patina 底之上的文字 |

铜绿是金的冷静对位：聚焦环、成功态、次要强调。**聚焦环用铜绿深，不用金**（沿用蓝本的 `--ks-focus-ring` 设定）。

### 2.4 朱（危险，vermilion）

| 令牌 | 值 | hex ≈ | 用途 |
|---|---|---|---|
| vermilion | `oklch(52% .16 35)` | `#B23B1D` | destructive、失败态、负向增量 |

### 2.5 警示（warning，派生）

| 令牌 | 值 | hex ≈ | 用途 |
|---|---|---|---|
| 古金墨 | `oklch(52% .09 78)` | ≈`#866B33` | 警示文字/图标（比 kinpaku-deep 更深以满足 4.5:1） |

### 2.6 仪器面板（深色面，instrument）

管理侧栏与代码/密钥展示区是「仪器」，永远深色，不随主题切换。

| 令牌 | 值 | hex ≈ | 用途 |
|---|---|---|---|
| instrument | `oklch(24% 0 0)` | `#1F1F1F` | 侧栏、深色面板底 |
| instrument-deep | `oklch(17% 0 0)` | `#0F0F0F` | 更深一档（滚动槽） |
| instrument-raised | `oklch(31% 0 0)` | `#303030` | 侧栏 hover/活动项底 |
| instrument-text | `oklch(93% 0 0)` | `#E8E8E8` | 面板文字 |
| instrument-muted | `oklch(68% 0 0)` | `#989898` | 面板次要文字、分组眉题 |
| instrument-rule | `oklch(100% 0 0 / 0.12)` | — | 面板内分割线 |
| instrument-edge | `oklch(100% 0 0 / 0.3)` | — | 面板内按钮描边 |

金在仪器面板上对比 7.5:1 —— **深色面板是金色文字唯一自由的地方**。

### 2.7 映射到 shadcn 变量（`frontend/src/styles.css` 的 `:root`）

实现时直接替换 `:root` 中对应变量，组件层基本不动：

```
--background:              oklch(97.8% 0 0)          /* paper        */
--foreground:              oklch(22% 0 0)            /* text         */
--card / --popover:        oklch(99.5% 0 0)          /* paper-raised */
--card/popover-foreground: oklch(22% 0 0)
--primary:                 oklch(13% 0 0)            /* ink —— 主按钮墨底 */
--primary-foreground:      oklch(98% 0 0)
--secondary:               oklch(95% 0 0)            /* paper-deep   */
--muted:                   oklch(95% 0 0)
--muted-foreground:        oklch(46% 0 0)
--accent:                  oklch(92% 0 0)            /* gray —— hover 底 */
--accent-foreground:       oklch(22% 0 0)
--destructive:             oklch(52% .16 35)         /* vermilion    */
--destructive-foreground:  oklch(98% 0 0)
--success:                 oklch(45% .1 190)         /* patina-deep  */
--success-foreground:      oklch(98% 0 0)
--warning:                 oklch(52% .09 78)         /* 古金墨（派生） */
--warning-foreground:      oklch(98% 0 0)
--info:                    oklch(70% .12 188)        /* patina —— 仅作底/描边 */
--info-foreground:         oklch(20% .04 190)        /* patina-ink   */
--border:                  oklch(13% 0 0 / 0.08)     /* rule         */
--input:                   oklch(13% 0 0 / 0.12)
--placeholder:             oklch(51% 0 0)            /* text-faint   */
--ring:                    oklch(45% .1 190)         /* patina-deep  */
--chart-1:                 oklch(77% .13 82)         /* kinpaku-rich */
--chart-2:                 oklch(70% .12 188)        /* patina       */
--chart-3:                 oklch(52% .16 35)         /* vermilion    */
--chart-4:                 oklch(61% .085 78)        /* kinpaku-deep */
--chart-5:                 oklch(51% 0 0)            /* text-faint   */
--sidebar:                 oklch(24% 0 0)            /* instrument   */
--sidebar-foreground:      oklch(93% 0 0)
--sidebar-primary:         oklch(84% .19 80.46)      /* kinpaku —— 活动指示条 */
--sidebar-primary-foreground: oklch(14% .018 95)     /* on-gold      */
--sidebar-accent:          oklch(31% 0 0)            /* raised       */
--sidebar-accent-foreground: oklch(93% 0 0)
--sidebar-border:          oklch(100% 0 0 / 0.12)
--sidebar-ring:            oklch(84% .19 80.46)
```

另需在 `styles.css` 增补扩展令牌（供细节场景引用）：

```
--gold: / --gold-rich: / --gold-deep:          （金三档）
--patina: / --patina-deep: / --patina-ink:     （铜绿三档）
--instrument: / --instrument-raised: / --instrument-text: / --instrument-muted:
--instrument-rule: / --instrument-edge:
--font-display:                                （Alumni Sans 栈）
```

**主题模式**：本规范以浅色「纸」模式为准（`color-scheme: light`）。侧栏与深色面板恒为深色。完整暗色模式不在当前范围；`.dark` 块留待后续按同一令牌体系派生。

**白标约束**：认证页支持运行时覆盖 `--primary` / `--ring`（自定义品牌色）。墨色是 `--primary` 的默认值而非硬编码——白标覆盖后按钮自然变为品牌色，本规范的所有其他规则不变。头像/身份类点缀用金系（`--gold-deep` on `--gold/15%`），不用 `--primary`。

---

## 3. 字体（Typography）

### 3.1 字族

| 角色 | 字体 | 加载 |
|---|---|---|
| UI/正文 | **Albert Sans**（400/500/600/700），中文回退 `PingFang SC` / `Microsoft YaHei` | Google Fonts |
| 展示/品牌/大数字 | **Alumni Sans**（200–600，窄身大字） | Google Fonts |
| 等宽 | **JetBrains Mono**（400/500）—— 已在 `index.html` 加载 | Google Fonts |

```
--font-sans:    'Albert Sans', 'PingFang SC', 'Microsoft YaHei', system-ui, sans-serif;
--font-display: 'Alumni Sans', 'Albert Sans', sans-serif;
--font-mono:    'JetBrains Mono', ui-monospace, 'SFMono-Regular', Consolas, monospace;
```

**Alumni Sans 只用于拉丁字符与数字**（品牌字「HERALD」、统计大数字）。中文没有对应的细字重系统回退，中文标题一律用 sans 栈 600。

### 3.2 字号刻度

| 级别 | 值 | 用途 |
|---|---|---|
| 展示 | 32–40px / 200（Alumni） | 统计卡数字、空状态大数字 |
| 页标题 | 20px / 600 / 墨 | PageHeader 标题 |
| 卡标题 | 15px / 600 | 卡片、对话框标题 |
| 小节标题 | 13px / 600 | 分组标题 |
| 正文/UI | 14px / 400–500（行高 1.6；中文正文行高 1.7） | 表单、表格、段落 |
| 辅助 | 12px / 400 / text-muted | 副标题、说明 |
| 眉题 eyebrow | 11px / 500 / text-muted / 大写 + 0.14em 字距（中文 0.1em，不大写） | 分组标签、统计卡标签、表头 |
| 等宽读数 | 12px / 400 / 0.02em | ID、密钥、代码、时间戳、IP |
| 品牌字 | 16–24px / 300–400 / 0.18em 字距 | 「HERALD」 |

眉题是本设计语言的签名元素：任何分组、卡片标签、统计标签都用眉题样式，而不是加粗小字。

### 3.3 等宽字的使用边界

用 mono：realm ID、client ID、API key、设备码、验证码、时间戳、IP、版本号、审计事件 ID、JSON。
不用 mono：邮箱、普通正文、按钮文字。等宽读数让控制台像仪表——但过度使用会变成终端模拟器。

---

## 4. 形状与尺寸（Shape & Size）

```
--radius:      0.5rem   (8px)   卡片、按钮、输入框、对话框
--radius-sm:   3px               微型芯片、行内代码、徽章
胶囊:          999px              pill 徽章、分段控件 thumb
```

控件高度（紧凑仪器风）：

| 档位 | 高度 | 用途 |
|---|---|---|
| xs | 26px | 表格内工具按钮、紧凑切换 |
| sm | 28px | 次要操作 |
| **md（默认）** | **32px** | 按钮、输入框、选择器 |
| lg | 44px | 认证页主 CTA、页面级唯一主操作 |

布局常量：侧栏 240px 固定；顶栏 56px；内容区内边距 24px；表单页内容最大宽度 `max-w-2xl`；营销/落地页区块间距 `clamp(72px, 8vw, 120px)`。

---

## 5. 阴影与分层（Elevation）

阴影全部基于墨色低透明度、多层叠加（不是单层黑块）：

```
lift-1（卡片常驻）:
  0 1px 1px oklch(13% 0 0/.05), 0 2px 3px oklch(13% 0 0/.04), 0 6px 12px oklch(13% 0 0/.05)
lift-2（弹层/悬停提升）:
  0 1px 1px oklch(13% 0 0/.04), 0 3px 5px oklch(13% 0 0/.05),
  0 12px 20px oklch(13% 0 0/.06), 0 32px 48px oklch(13% 0 0/.07)
key-lift（墨色主按钮）:
  0 1px 2px oklch(0% 0 0/.4)
cap-press（按钮按下）:
  inset 0 1px 2px oklch(13% 0 0/.14)
led-glow（LED 微光，仅深色面板）:
  0 0 0 1px oklch(13% 0 0/.12), 0 0 4px oklch(84% .19 80/.6)
```

映射：`--shadow-xs/--shadow-sm` ≈ lift-1；`--shadow-lg/--shadow-xl` ≈ lift-2；新增 `--shadow-key`、`--shadow-press`、`--shadow-led`。卡片描边永远保留发丝 rule——阴影是分层，描边是轮廓，两者不互替。

---

## 6. 动效（Motion）

```
--ease:   cubic-bezier(.2, .8, .2, 1)
--quick:  120ms   hover、焦点、开关
--settle: 200ms   抽屉、对话框、折叠、页面切换
```

- 按钮：hover 提亮 120ms；按下 120ms 内 `scale(.98)` + cap-press 内阴影。
- 抽屉/折叠沿用现有 200ms 动画，缓动统一为 `--ease`。
- `prefers-reduced-motion: reduce` 时全部退化为瞬时（现有 step-enter/exit 的处理方式为准）。
- 禁止：弹跳、过冲、无限脉冲、超过 250ms 的过渡。

---

## 7. 布局模式（Layout Patterns)

### 7.1 应用外壳（管理控制台 / 用户中心）

```
┌──────────┬──────────────────────────────┐
│          │ 顶栏 56px · 卡纸底 · 发丝下边  │
│ 侧栏      ├──────────────────────────────┤
│ 240px    │  主内容 · 纸底 · 24px 内边距   │
│ 仪器深色  │  （表单页 max-w-2xl 居中）     │
└──────────┴──────────────────────────────┘
```

- 侧栏 = 仪器面板：`instrument` 底，分组用 instrument-muted 眉题，活动项 `instrument-raised` 底 + **3px 金箔指示条**（左缘，全高），图标 18px、透明度 0.6→1 hover。
- 顶栏：卡纸底、发丝下边，右侧头像（金字 `--gold-deep` on `--gold/15%`）+ 用户菜单。
- 侧栏在两种主题下都是深色（现状保持）。

### 7.2 页面骨架（manage/user 页面统一）

`PageHeader`（标题 20/600 墨 + 副标题 13 muted + 右侧唯一主操作按钮）→ 卡片分区 → 表格或表单 → 对话框 CRUD。列表页必须有 skeleton / error / empty 三态组件（现有约定保持）。

### 7.3 认证页

纸底单列（max-w-md）：内容自顶部起排，全断点不垂直居中（顶部内边距移动 24px / 桌面 64px）。眉题（realm 名）→ 品牌字「HERALD」（Alumni 300 / 0.18em）→ 金色小分割线 → 表单 → 44px 墨色主 CTA。白标背景/主色覆盖逻辑不变。

### 7.4 深色仪器面板（代码/密钥场景）

API key 展示、设备码、审计事件 JSON、OAuth 原始配置：`instrument` 底、8px 圆角、mono 12px、instrument-text 文字；语法点缀——JSON 键 instrument-muted、值 instrument-text、关键值（密钥、设备码）**金色 + led-glow**。面板内按钮描边用 `instrument-edge`。

---

## 8. 组件规范（对应 `frontend/src/components/ui`）

| 组件 | 规范 |
|---|---|
| **Button** | primary：墨底纸字 + key-lift，右侧箭头/加号点缀可用金色；secondary：卡纸底 + 发丝描边；ghost：透明底 hover gray；destructive：vermilion。按下统一 `scale(.98)`。尺寸 26/32/44。每视图至多一个 primary。 |
| **Input / Select / Textarea** | 32px 高、8px 圆角、发丝描边、纸底；label 永远在上方（13px/500）；聚焦 = patina-deep 外环（2px + 25% 光晕）；ID/密钥类输入框用 mono。 |
| **Card** | paper-raised 底、发丝描边、8px、lift-1；可交互卡片 hover 升 lift-2；卡标题 15/600，卡内分组标签用眉题。 |
| **Badge / Pill** | 胶囊描边样式（非实心）：1px 语义色描边 + 语义色文字 + 6px 圆点，12px、26px 高；语义色只用 patina-deep / vermilion / 古金墨 / text-muted。实心徽章仅限金底 on-gold 字的「内置」标记类。 |
| **Table** | 表头 = 眉题样式；行高 44px；行分割发丝线；ID 列 mono；选中行 = paper-deep 底 + 左缘 2px 金条；行内操作用 26px ghost 按钮。 |
| **Tabs** | 分段控件（segmented）：paper-deep 轨道 + 卡纸活动 thumb + 墨字，8px；不用下划线式。 |
| **Switch** | 开 = 金色轨道 + 墨色滑块（金箔法则的展示位）；关 = gray-2 轨道 + 纸色滑块。 |
| **Dialog / Drawer** | 卡纸底、8px、lift-2、发丝描边；标题 15/600；底部操作区右对齐、一个 primary。 |
| **Alert** | 左缘 3px 语义色实线 + paper-raised 底 + 发丝整体描边；图标与标题同语义色。 |
| **Chart** | 系列色按 `--chart-1..5`（金/铜绿/朱/古金/灰）；网格发丝线；轴标签 11px text-faint；面积填充 12–18% 透明度；tooltip 卡纸底 + mono 数字。折线细于 2px 时金色系加深一档（kinpaku-deep）保对比。 |
| **Avatar** | 金系点缀：`--gold/15%` 底 + `--gold-deep` 首字母。 |
| **Skeleton** | gray-2 静态块（无脉冲），内容到达时 200ms 淡入。 |
| **Spinner** | 2px 细环，ink 20% 轨道 + ink 弧。 |
| **Empty state** | 眉题 + 一句说明 + 单个主操作（可选 Alma 大数字/图形点缀）。蒸馏原则的展示位。 |
| **链接** | 纸面链接：墨字 + rule 下划线，hover 下划线变金；深色面板内链接 instrument-text。 |

现有 `data-testid` 约定、cva 结构、`loading` prop、TanStack Form 绑定全部保持——本规范只改观感，不改行为与测试契约。

---

## 9. 中文（zh-CN）注意事项

- 眉题中文标签：11px、0.1em 字距、不大写（大写对中文无意义）。
- 中文正文行高 1.7（拉丁 1.6）；中文不用负字距。
- Alumni Sans 展示字只用于拉丁与数字；中文标题用 sans 栈 600。
- 等宽字体不含中文，mono 字段中出现的中文（如单位）自动回退 sans——可接受，不必强制。

---

*规范版本：1.0（2026-09-29）· 蓝本：impeccable.style 「kinpaku」 · 令牌源：`kinpaku-tokens.css`*
