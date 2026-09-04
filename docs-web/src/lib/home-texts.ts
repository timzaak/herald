export interface HomeTexts {
  badge: string;
  heroTitle: string;
  heroDesc: string;
  getStarted: string;
  liveDemo: string;
  terminal: {
    label: string;
    lines: {
      prefix?: string;
      text: string;
      status?: "ok" | "info" | "command";
    }[];
  };
  featureSectionTitle: string;
  featureSectionDesc: string;
  features: {
    title: string;
    desc: string;
    bullets: string[];
  }[];
  stepsSectionTitle: string;
  stepsSectionDesc: string;
  steps: {
    num: string;
    title: string;
    desc: string;
  }[];
  compareSectionTitle: string;
  compareSectionDesc: string;
  compareHeaders: {
    herald: string;
    auth0: string;
    supabase: string;
    keycloak: string;
  };
  compareRows: {
    label: string;
    herald: string;
    auth0: string;
    supabase: string;
    keycloak: string;
  }[];
  faqSectionTitle: string;
  faqSectionDesc: string;
  faq: {
    question: string;
    answer: string;
  }[];
  ctaTitle: string;
  ctaDesc: string;
  starGithub: string;
  footer: {
    copyright: string;
    privacy: string;
    terms: string;
  };
}

export const en: HomeTexts = {
  badge: "Open Source · AI-Customizable · Self-Hosted",
  heroTitle: "The open-source foundation for AI products",
  heroDesc:
    "Start with multi-tenant auth, billing, payments, credits, and an admin console. Use AI to shape the rest around your product—not your infrastructure.",
  getStarted: "Get Started",
  liveDemo: "Live Demo",
  terminal: {
    label: "terminal",
    lines: [
      {
        prefix: "$",
        text: "git clone https://github.com/timzaak/herald.git",
        status: "command",
      },
      { prefix: "$", text: "cd herald", status: "command" },
      { prefix: "$", text: "uv run scripts/demo-start.py", status: "command" },
      { text: "→ Starting PostgreSQL + Redis ...", status: "info" },
      { text: "✓ Database migrated", status: "ok" },
      { text: "✓ Multi-tenant auth  (RBAC, OAuth, TOTP)", status: "ok" },
      { text: "✓ Subscription billing (Stripe, Creem)", status: "ok" },
      { text: "✓ Admin console @ http://localhost:3000", status: "ok" },
      {
        text: "→ Your AI product infrastructure is ready. Build what makes it unique.",
        status: "info",
      },
    ],
  },
  featureSectionTitle: "The infrastructure around your AI product",
  featureSectionDesc:
    "Herald handles the shared product foundation so your team can spend its AI iterations on the logic customers actually pay for.",
  features: [
    {
      title: "Multi-Tenant Auth",
      desc: "Isolated Realms, each with its own users, roles, and OAuth providers.",
      bullets: [
        "Realm tenant isolation",
        "OAuth 2.0 providers",
        "TOTP & passkeys",
      ],
    },
    {
      title: "RBAC & Client Apps",
      desc: "Fine-grained roles per Realm, plus app registration with scoped credentials.",
      bullets: [
        "Per-Realm role permissions",
        "Client App registration",
        "Third-party API integration",
      ],
    },
    {
      title: "Billing & Payments",
      desc: "Subscription plans and one-time purchases, mapped to providers and Client Apps.",
      bullets: [
        "Plans & pricing tiers",
        "Stripe & Creem",
        "Invoices & entitlements",
      ],
    },
    {
      title: "Points & Credits",
      desc: "Multi-pool credit balances that meter product usage.",
      bullets: [
        "Multi-pool consumption",
        "Distribution & quota rules",
        "Per-user transaction history",
      ],
    },
    {
      title: "Consent & Compliance",
      desc: "Versioned Terms and Privacy Policies per Realm, hosted or external.",
      bullets: [
        "Version-bound re-consent",
        "Hosted or external pages",
        "Auditable account deletion",
      ],
    },
    {
      title: "MCP Server for AI Agents",
      desc: "Agents query your realm through permission-gated tools, not hand-written REST calls.",
      bullets: [
        "Five read-only tools",
        "Client API Key + RBAC",
        "Streamable HTTP at /mcp",
      ],
    },
  ],
  stepsSectionTitle: "From repository to AI product in three steps",
  stepsSectionDesc:
    "Deploy the foundation. Shape it with AI. Build your product logic.",
  steps: [
    {
      num: "01",
      title: "Deploy with Docker",
      desc: "Clone the repo, point your domain, and run dev-start.py. PostgreSQL, Redis, Caddy (with auto-TLS), and the Herald app start together on one machine.",
    },
    {
      num: "02",
      title: "Shape It with AI",
      desc: "Use AI-assisted development to adapt the open codebase, product flows, branding, roles, and integrations to your market.",
    },
    {
      num: "03",
      title: "Build Your Product Logic",
      desc: "Connect your application through Herald's APIs and OAuth 2.0 endpoints. Keep iterating on the experience that makes your AI product worth paying for.",
    },
  ],
  compareSectionTitle: "Why AI startup teams choose Herald",
  compareSectionDesc:
    "Go beyond authentication with billing, payments, credits, and product infrastructure in one AI-customizable, self-hosted system.",
  compareHeaders: {
    herald: "Herald",
    auth0: "Auth0",
    supabase: "Supabase",
    keycloak: "Keycloak",
  },
  compareRows: [
    {
      label: "Multi-tenant auth",
      herald: "Included",
      auth0: "Enterprise only",
      supabase: "Manual setup",
      keycloak: "Included",
    },
    {
      label: "Subscription billing",
      herald: "Built-in",
      auth0: "—",
      supabase: "—",
      keycloak: "—",
    },
    {
      label: "Points & credits",
      herald: "Built-in",
      auth0: "—",
      supabase: "—",
      keycloak: "—",
    },
    {
      label: "Self-hosted",
      herald: "Yes",
      auth0: "Cloud only",
      supabase: "Yes",
      keycloak: "Yes",
    },
    {
      label: "Open source",
      herald: "Apache-2.0",
      auth0: "No",
      supabase: "Partial",
      keycloak: "Apache-2.0",
    },
  ],
  faqSectionTitle: "Frequently asked questions",
  faqSectionDesc: "Everything you need to know about Herald.",
  faq: [
    {
      question: "What is Herald?",
      answer:
        "Herald is an open-source, self-hosted foundation for AI products. It gives startup teams multi-tenant auth, RBAC, billing, payments, credits, and an admin console in one codebase they can keep adapting with AI.",
    },
    {
      question: "How does Herald help me build an AI product faster?",
      answer:
        "Instead of spending early iterations stitching together auth, payment webhooks, entitlements, credits, and admin tools, you start with those systems connected. Your team can use AI-assisted development on the open codebase and focus on product logic.",
    },
    {
      question: "How is Herald different from an auth provider?",
      answer:
        "Herald is more than an authentication layer. It connects identity to subscriptions, one-time payments, entitlements, credits, invoices, and tenant administration, so paid access and product usage share one foundation.",
    },
    {
      question: "What does multi-tenant mean in Herald?",
      answer:
        "Multi-tenant means Herald organizes your users and data into isolated Realms. Each Realm is a separate tenant with its own users, OAuth providers, Client Apps, and billing plans. Data between Realms is fully isolated.",
    },
    {
      question: "How do I deploy Herald?",
      answer:
        "Herald deploys with Docker. You need a Linux server (Ubuntu 22.04+, 2GB RAM), Docker Engine 24+, and a domain. Four containers run together: the Herald app, PostgreSQL, Redis, and Caddy.",
    },
    {
      question: "What payment providers does Herald support?",
      answer:
        "Herald supports Stripe and Creem for subscription payments. You can create subscription plans with different pricing tiers, map plans to specific payment providers, and assign plans to Client Apps.",
    },
    {
      question: "What tech stack does Herald use?",
      answer:
        "Herald uses Rust (Axum framework) for the backend API and React with TypeScript for the frontend. Data is stored in PostgreSQL with SeaORM, and Redis handles sessions and caching.",
    },
    {
      question: "Is Herald free and open source?",
      answer:
        "Yes. Herald is released under the Apache-2.0 license. You can use, modify, and distribute it freely, including for commercial projects. There are no usage limits and no per-user fees.",
    },
  ],
  ctaTitle: "Build the AI product users pay for",
  ctaDesc:
    "Start with the open infrastructure around your product. Use AI to customize it, then focus every iteration on what makes your product unique.",
  starGithub: "Star on GitHub",
  footer: {
    copyright: "Herald · Apache 2.0",
    privacy: "Privacy",
    terms: "Terms",
  },
};

export const zh: HomeTexts = {
  badge: "开源 · AI 可定制 · 自托管",
  heroTitle: "AI 产品的开源基础设施",
  heroDesc:
    "多租户认证、计费、支付、积分和管理后台开箱即用。用 AI 围绕你的产品持续迭代，而不是反复搭建基础设施。",
  getStarted: "快速开始",
  liveDemo: "在线演示",
  terminal: {
    label: "terminal",
    lines: [
      {
        prefix: "$",
        text: "git clone https://github.com/timzaak/herald.git",
        status: "command",
      },
      { prefix: "$", text: "cd herald", status: "command" },
      { prefix: "$", text: "uv run scripts/demo-start.py", status: "command" },
      { text: "→ 正在启动 PostgreSQL + Redis ...", status: "info" },
      { text: "✓ 数据库迁移完成", status: "ok" },
      { text: "✓ Multi-tenant 认证 (RBAC, OAuth, TOTP)", status: "ok" },
      { text: "✓ 订阅计费 (Stripe, Creem)", status: "ok" },
      { text: "✓ 管理后台 @ http://localhost:3000", status: "ok" },
      {
        text: "→ AI 产品基础设施已就绪，开始构建你的独特价值。",
        status: "info",
      },
    ],
  },
  featureSectionTitle: "AI 产品周围的基础设施",
  featureSectionDesc:
    "Herald 处理共性的产品底座，让团队把每一次 AI 迭代都用在客户真正愿意付费的产品逻辑上。",
  features: [
    {
      title: "多租户认证",
      desc: "相互隔离的 Realm，每个 Realm 拥有独立的用户、角色和 OAuth 提供商。",
      bullets: ["租户数据隔离", "OAuth 2.0 提供商", "TOTP 与 Passkey"],
    },
    {
      title: "RBAC 与客户端应用",
      desc: "每个 Realm 内细粒度的角色权限，应用注册后获得受限凭证。",
      bullets: ["Realm 级角色权限", "客户端应用注册", "第三方 API 集成"],
    },
    {
      title: "计费与支付",
      desc: "订阅计划与一次性付款，映射到支付提供商并分配给客户端应用。",
      bullets: ["计划与定价层级", "Stripe 与 Creem", "发票与权益"],
    },
    {
      title: "积分与信用点",
      desc: "多池积分余额，精确计量产品用量。",
      bullets: ["多池消耗", "发放与配额规则", "用户交易明细"],
    },
    {
      title: "知情同意与合规",
      desc: "按 Realm 版本化的用户协议与隐私政策，可托管也可外链。",
      bullets: ["版本绑定的重新同意", "托管或外部页面", "可审计的账户注销"],
    },
    {
      title: "面向 AI Agent 的 MCP 服务器",
      desc: "Agent 通过受权限约束的工具查询你的租户，无需手写 REST 调用。",
      bullets: [
        "五个只读工具",
        "Client API Key + RBAC",
        "/mcp Streamable HTTP",
      ],
    },
  ],
  stepsSectionTitle: "三步从代码仓库到 AI 产品",
  stepsSectionDesc: "部署底座，用 AI 塑造，构建产品逻辑。",
  steps: [
    {
      num: "01",
      title: "Docker 部署",
      desc: "克隆仓库，指向你的域名，运行 dev-start.py。PostgreSQL、Redis、Caddy（自动 TLS）与 Herald 应用在同一台机器上启动。",
    },
    {
      num: "02",
      title: "用 AI 塑造产品",
      desc: "通过 AI 辅助开发调整开放代码、产品流程、品牌、角色和集成方式，使底座适配你的市场。",
    },
    {
      num: "03",
      title: "构建产品逻辑",
      desc: "通过 Herald 的 API 与 OAuth 2.0 端点接入应用，持续迭代真正让用户愿意付费的 AI 产品体验。",
    },
  ],
  compareSectionTitle: "AI 创业团队为何选择 Herald",
  compareSectionDesc:
    "不止于认证：在一个 AI 可定制的自托管系统中获得计费、支付、积分和完整产品基础设施。",
  compareHeaders: {
    herald: "Herald",
    auth0: "Auth0",
    supabase: "Supabase",
    keycloak: "Keycloak",
  },
  compareRows: [
    {
      label: "多租户认证",
      herald: "内置",
      auth0: "企业版",
      supabase: "需手动配置",
      keycloak: "内置",
    },
    {
      label: "订阅计费",
      herald: "内置",
      auth0: "—",
      supabase: "—",
      keycloak: "—",
    },
    {
      label: "积分与信用点",
      herald: "内置",
      auth0: "—",
      supabase: "—",
      keycloak: "—",
    },
    {
      label: "自托管",
      herald: "支持",
      auth0: "仅云",
      supabase: "支持",
      keycloak: "支持",
    },
    {
      label: "开源",
      herald: "Apache-2.0",
      auth0: "否",
      supabase: "部分",
      keycloak: "Apache-2.0",
    },
  ],
  faqSectionTitle: "常见问题",
  faqSectionDesc: "关于 Herald 你需要知道的一切。",
  faq: [
    {
      question: "Herald 是什么？",
      answer:
        "Herald 是 AI 产品的开源、自托管基础设施。它让创业团队从同一套可持续用 AI 改造的代码出发，直接获得多租户认证、RBAC、计费、支付、积分和管理后台。",
    },
    {
      question: "Herald 如何帮助我更快构建 AI 产品？",
      answer:
        "你不必把早期迭代花在拼接认证、支付 webhook、权益、积分和管理工具上。Herald 已将这些系统连在一起，团队可以用 AI 改造开放代码，把时间用在自己的产品逻辑上。",
    },
    {
      question: "Herald 与单纯的认证服务有何不同？",
      answer:
        "Herald 不只是认证层。它把身份与订阅、一次性支付、权益、积分、发票和租户管理连接起来，让付费访问与产品用量共享同一套底座。",
    },
    {
      question: "Herald 中的多租户是什么意思？",
      answer:
        "多租户意味着 Herald 将用户和数据组织到相互隔离的 Realm 中。每个 Realm 是独立的租户，拥有自己的用户、OAuth 提供商、客户端应用和计费计划。Realm 之间的数据完全隔离。",
    },
    {
      question: "如何部署 Herald？",
      answer:
        "Herald 通过 Docker 部署。你需要一台 Linux 服务器（Ubuntu 22.04+，2GB 内存）、Docker Engine 24+ 和一个域名。四个容器一起运行：Herald 应用、PostgreSQL、Redis 和 Caddy。",
    },
    {
      question: "Herald 支持哪些支付提供商？",
      answer:
        "Herald 支持 Stripe 和 Creem 用于订阅付款。你可以创建不同定价层级的订阅计划，将其映射到特定支付提供商，并分配给客户端应用。",
    },
    {
      question: "Herald 使用什么技术栈？",
      answer:
        "Herald 后端使用 Rust（Axum 框架）提供 API，前端使用 React + TypeScript。数据存储在 PostgreSQL 中，使用 SeaORM；Redis 负责会话与缓存。",
    },
    {
      question: "Herald 是否免费开源？",
      answer:
        "是的。Herald 采用 Apache-2.0 许可证发布。你可以自由使用、修改和分发，包括商业项目。没有使用限制，也不按用户收费。",
    },
  ],
  ctaTitle: "构建用户愿意付费的 AI 产品",
  ctaDesc:
    "从开源产品基础设施开始，用 AI 持续定制，把每一次迭代都用在真正让产品与众不同的地方。",
  starGithub: "Star on GitHub",
  footer: {
    copyright: "Herald · Apache 2.0",
    privacy: "隐私政策",
    terms: "服务条款",
  },
};
