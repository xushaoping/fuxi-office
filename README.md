# 伏羲智能决策工作台

企业级 DMN 决策自动化平台，全栈 Rust 实现。用户以自然语言描述业务规则，系统通过 LLM 智能体生成结构化 DSL，经多层校验后编译为标准 DMN 1.5 XML，并提供完整的版本管理、业务验收、发布运行和数据分析能力。

## 核心特性

### 智能建模
- **自然语言驱动**：无需掌握 DSL、DMN 或 FEEL 语法，直接用业务语言描述决策逻辑
- **阶段化工作流**：需求分析 → 澄清确认 → 决策方案 → 模型生成，每步可控可验证
- **增量修改**：对已有模型进行结构化补丁式修改，保留未受影响的元素和引用
- **知识库集成**：绑定企业术语、字段目录和制度文档，自动注入建模上下文
- **流式反馈**：实时展示 LLM 推理和生成进度，支持中断和错误自动修复

### 决策引擎
- **完整 DMN 1.5 实现**：支持决策表（全部命中策略）、字面量表达式、上下文、关系、条件、过滤器、迭代器、函数调用
- **生产级 FEEL 解释器**：完整实现 FEEL 表达式语言，包含算术、逻辑、字符串、日期时间、列表和上下文操作
- **执行轨迹**：确定性记录依赖执行顺序、逻辑类型、命中规则、中间结果和最终输出
- **决策服务封装**：已发布版本中的每个 DecisionService 自动成为可调用的 HTTP 端点；未定义服务时按终端决策生成默认服务

### 质量保障
- **多层校验门禁**：DSL 结构 → DMN 编译 → FEEL 语法 → 依赖完整性 → 契约测试
- **自动验收测试**：边界值、缺失输入、类型错误、数值阈值的下界/等值/上界案例生成
- **静态分析**：规则冲突检测（UNIQUE/ANY）、不可达规则（FIRST）、覆盖缺口识别
- **变更审查**：版本差异分析，风险等级评估（低/中/高），人工确认后保存
- **业务验收**：必须添加并批准至少一个业务案例，所有门禁通过后才能发布

### 生产运营
- **版本化部署**：不可变版本历史，发布时自动将上一版本标记为已退役
- **运行监控**：最近 50 条执行记录持久化，含输入、输出、轨迹和耗时
- **业务反馈闭环**：回填实际业务结果、偏差判定和说明，作为后续优化数据源
- **数据分析**：按当前发布版本隔离分析最近 30 天运行数据，覆盖成功率、偏差率、规则命中、耗时分布和异常检测
- **优化建议**：基于运行数据自动生成结构化改进提案（当前为占位实现）

### 协作与治理
- **项目协作**：基于角色的访问控制（所有者/建模者/复核者/审批者/查看者）
- **版本回滚**：从已发布或已退役版本创建回滚草稿，重新验收后发布
- **模型库**：保存个人模板快照，复用平台内置模板或历史已发布模型
- **审计日志**：记录项目创建、版本变更、验收批准、发布退役、服务调用等所有关键操作
- **知识溯源**：版本创建时固化引用的知识源元数据快照，后续变更不影响历史版本

完整的业务操作流程、发布门禁和常见问题见 [用户操作手册](docs/user-operation-manual.md)。

知识库录入、项目绑定、自然语言建模关联和 DSL 示例见 [知识库与建模关联示例](docs/knowledge-base-examples.md)。

## 技术架构

### 技术栈
- **后端**：Axum 0.8、Tokio 异步运行时、SQLx + PostgreSQL、JWT 认证
- **前端**：Leptos 0.8 CSR、WebAssembly、leptos_router、gloo-storage
- **决策引擎**：项目内置 DMN 1.5 / FEEL 完整实现（解析、编译、运行时）
- **LLM 集成**：OpenAI Chat Completions 兼容接口，支持流式 SSE 和工具调用
- **构建工具**：cargo-leptos 协调前后端编译，Rust 2024 edition

### 代码结构

```text
src/
  main.rs                    后端入口：Axum 服务器、路由和中间件
  lib.rs                     前端入口：Leptos 根组件
  components.rs              Leptos UI 组件（登录、工作台、测试面板等）
  api.rs                     API 端点实现和 SSE 流式管道
  auth.rs                    用户认证、JWT 签发与中间件
  types.rs                   共享数据结构（AppState、Claims 等）

  dmn/                       DMN 1.5 引擎实现
    model.rs                   DMN 元模型（Decision、DecisionTable、ItemDefinition 等）
    xml.rs                     DMN XML 解析器（quick-xml）
    compile.rs                 类型解析、依赖图分析、FEEL 预编译
    runtime.rs                 拓扑执行引擎，支持全部命中策略
    diagnostics.rs             结构校验、依赖循环检测
    artifact.rs                序列化和持久化
    tests.rs                   单元测试

  feel/                      FEEL 表达式语言实现
    lexer.rs                   词法分析器
    parser.rs                  递归下降语法分析器
    ast.rs                     抽象语法树定义
    interpreter.rs             树遍历求值器
    builtins.rs                FEEL 标准库（列表、字符串、日期、聚合）
    value.rs                   强类型值系统
    tests.rs                   表达式测试套件

  nl_dsl/                    自然语言到 DSL 的智能体系统
    agent.rs                   LLM 流式客户端、工具调用、压缩处理
    dsl.rs                     结构化 DSL 定义和完整校验
    dmn.rs                     DSL → DMN XML 编译器
    prompts.rs                 多阶段提示词工程

  projects.rs                决策项目 CRUD 和版本管理
  workflow.rs                需求分析、决策方案工作流数据结构
  workflow_api.rs            工作流 API 端点（分析、规划、确认）
  knowledge.rs               知识源管理和上下文注入
  models.rs                  数据库模型定义
  version_verification.rs    自动验收测试生成和执行
  version_review.rs          版本差异分析和风险评估
  execution_trace.rs         决策执行轨迹构建
  decision_analytics.rs      运行数据分析和异常检测
  project_access.rs          基于角色的访问控制

script/
  init.sql                   数据库初始化脚本（用户表、基础表）
  002-019_*.sql              增量升级脚本（项目、版本、测试、服务、知识库和用户角色等）
  build-release.sh           生产构建脚本

public/style/                工作台样式表
docs/                        设计文档和操作手册
```

### 部署架构

- **单一二进制**：后端服务器内嵌静态资源，同时提供 API 和前端
- **CSR 模式**：Leptos 客户端渲染，首屏由服务端返回 HTML 外壳，后续由 WASM 接管
- **静态部署支持**：`target/site/` 可作为独立静态站点部署（API 需单独部署）
- **数据库**：PostgreSQL 14+，初始化脚本覆盖用户、项目、版本、测试、服务、知识库和审计等核心表

## 快速开始

### 前置条件

- Rust 1.88（仓库工具链固定版本；`Cargo.toml` 的最低版本要求为 Rust 1.85，项目使用 Rust 2024 edition）
- PostgreSQL 14 或更高版本
- `wasm32-unknown-unknown` Rust target
- `cargo-leptos`

```bash
rustup target add wasm32-unknown-unknown
cargo install cargo-leptos --version 0.3.7 --locked
```

### 环境配置

复制 `.env.example` 为 `.env`，再填写运行参数（不要提交真实配置）：

```bash
cp .env.example .env
```

`.env` 的关键配置示例：

```dotenv
# Docker Compose 使用的 PostgreSQL 密码；手工连接已有 PostgreSQL 时可按实际配置填写
DB_PASSWORD=<数据库密码>

# PostgreSQL 连接信息；数据库名称需与 script/init.sql 一致
DATABASE_URL=postgres://<用户名>:<密码>@127.0.0.1:5432/fuxi_office

# 至少使用 32 位随机字符串
JWT_SECRET=<随机且保密的 JWT 密钥>

# 超级管理员 admin 的初始密码（至少 6 位；生产环境必须显式设置）
ADMIN_PASSWORD=<admin 初始密码>

# 后端监听地址，未设置时为 127.0.0.1:3000
BIND_ADDR=127.0.0.1:3000

# 环境标识。开发环境的数据库错误会返回详细原因；生产环境仅返回通用错误
APP_ENV=development

# OpenAI 兼容接口；未设置 OPENAI_API_KEY 时仍可启动，但无法生成模型
OPENAI_API_KEY=<API 密钥>
OPENAI_BASE_URL=https://api.openai.com/v1
OPENAI_MODEL=gpt-4o-mini

# 可选：传递给兼容接口的额外 JSON 请求字段。仅在当前服务商文档支持时设置。
# 例如支持 enable_thinking 的服务可关闭推理，以缩短工具调用前的等待时间。
OPENAI_REQUEST_OPTIONS_JSON='{"enable_thinking":false}'

# 可选：按智能任务覆盖模型和请求参数；未设置时均回退到 OPENAI_MODEL 与
# OPENAI_REQUEST_OPTIONS_JSON。密钥和接口地址始终使用上面的服务端全局配置。
OPENAI_ANALYSIS_MODEL=<需求分析模型>
OPENAI_ANALYSIS_REQUEST_OPTIONS_JSON='{"enable_thinking":true}'
OPENAI_PLAN_MODEL=<方案规划模型>
OPENAI_PLAN_REQUEST_OPTIONS_JSON='{"enable_thinking":true}'
OPENAI_DSL_MODEL=<DSL生成模型>
OPENAI_DSL_REQUEST_OPTIONS_JSON='{"enable_thinking":false}'
# 增量修改的变更意图生成模型。该任务会由服务端强制关闭 thinking。
OPENAI_CHANGE_INTENT_MODEL=<增量修改模型>
OPENAI_CHANGE_INTENT_REQUEST_OPTIONS_JSON='{"enable_thinking":false}'
```

### 数据库初始化

创建数据库和表结构：

```bash
# 首次部署时创建应用数据库角色；已存在时跳过此命令
psql -U <管理员用户名> -d postgres -c "CREATE USER fuxi WITH PASSWORD '<数据库密码>';"

# init.sql 会创建 fuxi_office 数据库并初始化表结构
psql -U <管理员用户名> -d postgres -v app_user=fuxi -f script/init.sql
```

`<数据库密码>` 必须与 `.env` 中 `DATABASE_URL` 使用的密码一致。`script/init.sql` 会创建 `fuxi_office` 数据库、用户表，以及决策项目、版本、制品和智能运行记录表。

**已有数据库升级**（按顺序执行所需脚本）：

```bash
# 决策项目和版本管理
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/002_decision_projects.sql

# 需求工作流
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/003_decision_workflow.sql

# 增量修改和补丁审计
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/004_model_patch.sql

# 版本差异和变更确认
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/005_version_review.sql

# 业务验收测试套件
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/006_decision_test_cases.sql

# 执行轨迹持久化
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/007_execution_trace.sql

# 决策服务运行记录
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/008_decision_services.sql

# 版本回滚和审计日志
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/009_version_rollback_audit.sql

# 模型库和模板复用
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/010_model_library.sql

# 知识库和术语字典
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/011_knowledge_sources.sql

# 业务结果反馈闭环
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/012_decision_run_outcomes.sql

# 优化建议
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/013_optimization_proposals.sql

# 项目协作成员
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/014_project_members.sql

# 模型变更候选（预览/提交）
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/015_model_change_candidates.sql

# revision 并发控制字段
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/016_model_revisions.sql

# 协作成员多角色支持（已有数据库必须执行）
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/017_project_member_roles.sql

# 对外决策服务调用凭证
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/018_decision_service_api_keys.sql

# 系统用户角色（已有数据库必须执行）
psql -U <管理员> -d fuxi_office -v app_user=fuxi -f script/019_user_roles.sql
```

**权限问题修复**：

如果表已由管理员创建但登录或用户管理时报 `permission denied for table users`，使用管理员账号补授权：

```sql
GRANT USAGE ON SCHEMA public TO fuxi;
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE
    public.users,
    public.decision_projects,
    public.decision_versions,
    public.decision_artifacts,
    public.decision_test_cases,
    public.decision_runs,
    public.decision_optimization_proposals,
    public.audit_events,
    public.decision_templates,
    public.knowledge_sources,
    public.project_knowledge_sources,
    public.knowledge_audit_events,
    public.decision_workflows,
    public.agent_runs,
    public.project_members,
    public.model_change_candidates,
    public.decision_service_api_keys
TO fuxi;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO fuxi;
```

### 本地开发

启动带前端热更新的开发服务：

```bash
cargo leptos watch
```

服务默认监听 `http://127.0.0.1:3000`。首次启动会根据 `ADMIN_PASSWORD` 自动创建
超级管理员 `admin`，登录后由管理员在“用户管理”页面添加普通用户并分配角色。

**分离检查**：

```bash
# 后端检查
cargo check

# 前端 WASM library 检查
cargo check --lib --target wasm32-unknown-unknown --features csr

# 运行测试（DMN 引擎、FEEL 解释器、DSL 校验）
cargo test --lib
```

## 业务流程

### 项目工作流

工作台按成员角色执行权限控制，角色在服务端实时生效：

| 角色 | 权限 |
|------|------|
| 所有者 | 管理成员、建模、查看运行数据；不能代替独立复核或发布审批 |
| 建模者 | 需求分析、方案、模型和版本草稿；生成、确认或驳回优化建议；回填实际业务结果 |
| 复核者 | 独立复核模型与批准业务案例 |
| 审批者 | 在复核和验收完成后发布版本 |
| 查看者 | 查看项目、版本、服务、运行记录、分析和审计，不能修改数据 |

> **重要**：系统中的"确认""批准"和"发布"动作均使用当前登录账号记录。正式环境应为每位操作者分配独立账号，不要共用账号。
> 同一账号可以拥有多个项目角色，但不能同时作为同一版本的独立复核人和发布审批人。

### 完整决策生命周期

```text
需求分析 → 澄清确认 → 决策方案 → 模型生成/修改 → 变更审查确认
       → 业务验收测试 → 自动验收门禁 → 发布服务
       → 运行调用 → 业务结果回填 → 数据分析 → 优化建议
```

**新项目建模**：
1. 输入业务需求，系统分析目标、输入输出和关键歧义
2. 回答澄清问题，系统生成决策方案
3. 确认方案后，系统生成 DSL 和 DMN（自动校验和错误修复）
4. 审查版本差异和风险摘要，确认后保存草稿版本

**增量修改**：
1. 已有草稿项目进入"应用修改"模式
2. 描述需要改变的内容，系统生成结构化 ModelPatch
3. 服务端应用最小变更并执行完整校验
4. 审查差异后确认保存

**验收发布**：
1. 系统自动执行技术验收（边界、缺失输入、类型错误、阈值测试）
2. 添加至少一个业务验收案例，填写输入和期望输出
3. 批量运行案例，复核者逐项批准
4. 所有门禁通过后，审批者发布版本

**生产运行**：
1. 已发布版本中的每个 DecisionService 自动成为 HTTP 端点
2. 调用时携带业务关联号（订单号、申请号等）
3. 执行完成后回填实际业务结果和偏差判定
4. 系统分析最近 30 天运行数据，识别异常模式

**持续改进**：
1. 查看数据分析仪表板（规则覆盖、偏差率、耗时趋势）
2. 系统基于异常检测生成优化建议（占位功能）
3. 建模者确认后创建新版本，进入正常验收流程

### 知识库集成

项目可绑定企业术语、字段目录和制度政策文档：

1. 在"知识库"维护知识源（名称、类型、版本、生效/失效日期、来源地址、正文）
2. 显式绑定到项目
3. 只有当前用户拥有、已绑定、状态为启用且处于有效期内的知识会注入建模上下文
4. 创建版本时服务端固化知识源元数据快照，后续变更不影响历史版本

### 模型库复用

"模型库"统一展示三类来源：

1. **平台内置模板**：行业标准决策模型
2. **个人模板**：用户保存的模板快照
3. **历史模型**：当前用户已发布或已退役的版本

复用任一来源都会：
- 原子创建独立项目和 v1 草稿
- 复制 DSL、DMN、需求方案和业务案例
- 不继承案例执行、批准、验收或发布状态

### 版本回滚

从已发布或已退役版本创建回滚草稿：

1. 选择目标版本，输入回滚原因
2. 系统复制版本制品和业务案例，清空执行与批准状态
3. 按正常门禁重新验收和发布
4. 回滚来源和原因保存在版本元数据和审计日志中

## 智能体建模详解

### 生成流程

登录后，在左侧输入业务需求并点击“生成模型”。系统按以下顺序处理：

1. **需求分析**：LLM 分析业务目标、输入输出和关键歧义，提出澄清问题
2. **决策方案**：根据用户回答生成决策分解方案
3. **DSL 生成**：LLM 调用 `generate_dmn_dsl` 工具返回结构化 DSL
4. **DSL 校验**：验证名称、引用、类型、决策表列数和依赖关系
5. **DMN 编译**：转换为 DMN 1.5 XML
6. **FEEL 解析**：内置引擎解析和编译所有 FEEL 表达式
7. **错误修复**：任一步失败都会将具体错误反馈给 LLM，要求基于上一版模型修复后重新生成（最多 4 次尝试）
8. **返回结果**：只有 DSL、DMN 和 FEEL 都通过核验时，才将 YAML 和 XML 返回工作台

工作台会展示执行过程，右侧可在 `DSL YAML` 与 `DMN XML` 两个页签间切换查看产物。

> **注意**：这项核验保证模型结构和 FEEL 语法可被引擎编译，不替代业务人员对规则阈值、命中策略和最终业务结果的审核。可点击顶部“核验 DMN”或“测试模型”进行后续检查。

### LLM 配置优化

#### 缩短首段等待时间

默认请求不会附加任何厂商私有的推理参数，确保 OpenAI 兼容接口可以直接使用。若所用模型会先输出较长推理，再发起工具调用，可优先改用该服务商的非推理模型；也可在 `.env` 设置 `OPENAI_REQUEST_OPTIONS_JSON` 传入服务商明确支持的禁用推理选项：

```bash
OPENAI_REQUEST_OPTIONS_JSON='{"enable_thinking":false}'
```

该参数会同时用于普通与 SSE 请求。系统不会将推理文本展示给终端用户，但如果流中出现 `reasoning_content`，执行过程会更新为“模型正在推理，准备结构化 DSL”，不再持续显示“等待首段输出”。

如果接口因该额外参数返回 400，请删除此配置或改用服务商文档指定的字段和值。

#### 按阶段覆盖模型

部署也可按智能任务阶段覆盖模型：

```bash
# 需求分析（可选推理）
OPENAI_ANALYSIS_MODEL=<分析模型>
OPENAI_ANALYSIS_REQUEST_OPTIONS_JSON='{"enable_thinking":true}'

# 方案规划（可选推理）
OPENAI_PLAN_MODEL=<规划模型>
OPENAI_PLAN_REQUEST_OPTIONS_JSON='{"enable_thinking":true}'

# DSL 生成（工具调用稳定优先）
OPENAI_DSL_MODEL=<生成模型>
OPENAI_DSL_REQUEST_OPTIONS_JSON='{"enable_thinking":false}'

# 增量修改的变更意图（工具调用稳定优先）
OPENAI_CHANGE_INTENT_MODEL=<变更意图模型>
OPENAI_CHANGE_INTENT_REQUEST_OPTIONS_JSON='{"enable_thinking":false}'
```

每组分别支持 `MODEL` 与 `REQUEST_OPTIONS_JSON`；遗漏、空值或非 JSON 对象的阶段配置都会回退到全局配置。一般可为分析和规划选择推理能力更强的模型，为 DSL 和补丁选择结构化工具调用更稳定的模型。

工作流的 `agent_runs` 会保存每次分析或规划实际采用的模型和阶段提示词版本。

> **重要**：增量修改会先生成结构化变更意图，再由服务端应用补丁。该阶段的请求会统一强制关闭 thinking（覆盖自定义配置），避免模型只输出推理文本而不返回可处理的结构化结果。DSL 生成仍会根据模型能力省略或启用函数级 `tool_choice`。

### 编写需求的建议

- **明确输入字段**：类型、单位和缺省处理，例如“年收入，单位元，number”
- **指出优先级**：互斥规则适合 `UNIQUE`，存在兜底或优先匹配规则时应说明使用 `FIRST`
- **说明依赖关系**：例如“最终审批依赖风险等级和债务收入比”
- **FEEL 语法**：字符串使用双引号；条件、阈值、区间和兜底规则应完整给出
- **增量修改**：对已有模型使用“应用修改”模式，只描述需要改变的内容；智能体会保留未受影响的元素和引用

### 复杂示例：零售贷款审批

下面的内容可直接作为一次新建模型的输入。它同时覆盖多个输入、公式型决策、决策表、决策依赖、优先命中规则和最终输出：

```text
创建一个名为 RetailLoanApproval 的零售贷款审批模型。

输入：ApplicantAge（number，岁）、AnnualIncome（number，年收入元）、
CreditScore（number）、MonthlyDebtPayment（number，月债务支出元）、
RequestedAmount（number，申请金额元）、EmploymentMonths（number，当前工作月数）、
ExistingCustomer（boolean）。

先创建 DebtToIncomeRatio 决策，计算 MonthlyDebtPayment * 12 / AnnualIncome，结果为 number。

再创建 RiskGrade 决策，使用决策表，输入 CreditScore、EmploymentMonths 和 DebtToIncomeRatio，
输出为 string。规则按 FIRST 顺序：
1. CreditScore 小于 600，或 DebtToIncomeRatio 大于 0.55，输出 “High”；
2. CreditScore 小于 720，或 EmploymentMonths 小于 12，输出 “Medium”；
3. 其他情况输出 “Low”。

创建 Eligibility 决策，使用 FIRST 决策表，依赖 ApplicantAge、AnnualIncome、RequestedAmount：
1. ApplicantAge 小于 18，输出 “Reject”；
2. AnnualIncome 小于 30000，输出 “Reject”；
3. RequestedAmount 大于 AnnualIncome 的 3 倍，输出 “ManualReview”；
4. 其他情况输出 “Eligible”。

最后创建 LoanDecision 决策，依赖 RiskGrade、Eligibility 和 ExistingCustomer，并使用 FIRST 决策表：
1. Eligibility 为 “Reject”，输出 “Reject”；
2. RiskGrade 为 “High”，输出 “Reject”；
3. Eligibility 为 “ManualReview”，输出 “ManualReview”；
4. RiskGrade 为 “Medium” 且 ExistingCustomer 为 false，输出 “ManualReview”；
5. 其他情况输出 “Approve”。

所有决策设置正确的 requiredInputs 或 requiredDecisions，字符串输出使用合法 FEEL 字面量；不需要 diagram、BKM 或 decision service。
```

模型生成后，打开“测试模型”，可用下面的输入验证依赖链和最终结果：

```json
{
  "ApplicantAge": 32,
  "AnnualIncome": 120000,
  "CreditScore": 760,
  "MonthlyDebtPayment": 1800,
  "RequestedAmount": 200000,
  "EmploymentMonths": 48,
  "ExistingCustomer": true
}
```

若要进行增量修改，可切换到“应用修改”模式并输入：

```text
将 RiskGrade 的高风险债务收入比阈值从 0.55 调整为 0.50，同时保留其余决策和依赖关系。
```

## API 概览

所有接口均以 `/api` 为前缀。除登录、健康检查和对外决策服务调用外，其余接口需要 `Authorization: Bearer <token>`。对外调用使用受服务范围限制的 `X-Decision-Key`，不接受 JWT 代替该凭证。

### 核心接口

| 方法 | 路径 | 用途 |
|------|------|------|
| `GET` | `/api/health` | 健康检查 |
| `POST` | `/api/login` | 登录并获取 JWT |
| `GET` | `/api/me` | 获取当前用户 |
| `GET/POST` | `/api/admin/users` | 管理员查看或添加系统用户 |
| `PUT` | `/api/admin/users/{username}` | 管理员修改用户角色或密码 |
| `DELETE` | `/api/admin/users/{username}` | 管理员删除系统用户 |

### 项目管理

| 方法 | 路径 | 用途 |
|------|------|------|
| `GET/POST` | `/api/projects` | 读取或创建决策项目 |
| `GET` | `/api/projects/{id}` | 读取项目最近草稿 |
| `GET/POST` | `/api/projects/{id}/versions` | 读取或创建项目版本 |
| `POST` | `/api/projects/{id}/model-changes/preview` | 预览增量修改候选 |
| `POST` | `/api/projects/{id}/model-changes/{cid}/commit` | 提交增量修改候选 |
| `DELETE` | `/api/projects/{id}` | 删除项目 |
| `GET/PUT/DELETE` | `/api/projects/{id}/members[/{username}]` | 查看、维护项目成员 |
| `GET` | `/api/projects/{id}/audit-events` | 查看审计记录 |

### 工作流（需求分析和方案规划）

| 方法 | 路径 | 用途 |
|------|------|------|
| `GET` | `/api/projects/{id}/workflow` | 读取需求与方案工作流 |
| `POST` | `/api/projects/{id}/workflow/analyze-stream` | 流式分析决策需求 |
| `POST` | `/api/projects/{id}/workflow/analyze` | 分析决策需求 |
| `POST` | `/api/projects/{id}/workflow/plan` | 根据澄清回答生成决策方案 |
| `POST` | `/api/projects/{id}/workflow/confirm` | 确认决策方案 |

### 模型生成和验证

| 方法 | 路径 | 用途 |
|------|------|------|
| `POST` | `/api/run` | 流式生成、校验并编译 DMN |
| `POST` | `/api/generate` | 非流式生成 DSL |
| `POST` | `/api/generate-change-intent` | 生成增量修改意图 |
| `POST` | `/api/validate` | 校验 DSL |
| `POST` | `/api/compile` | 将 DSL 编译为 DMN XML |
| `POST` | `/api/validate-dmn` | 核验 DMN XML |
| `POST` | `/api/analyze-inputs` | 分析 DMN 执行输入模板 |
| `POST` | `/api/evaluate` | 执行 DMN 测试 |

### 版本验收与发布

| 方法 | 路径 | 用途 |
|------|------|------|
| `GET/POST` | `/api/projects/{id}/versions/{vid}/verification` | 查看或执行自动验收 |
| `GET/POST` | `/api/projects/{id}/versions/{vid}/test-cases` | 查看或创建业务案例 |
| `POST` | `/api/projects/{id}/versions/{vid}/test-cases/run` | 批量运行案例 |
| `POST` | `/api/projects/{id}/versions/{vid}/test-cases/generate` | 自动生成候选案例 |
| `PUT` | `/api/projects/{id}/versions/{vid}/test-cases/{cid}/approval` | 批准案例 |
| `DELETE` | `/api/projects/{id}/versions/{vid}/test-cases/{cid}` | 删除案例 |
| `POST` | `/api/projects/{id}/versions/{vid}/publish` | 发布版本 |
| `POST` | `/api/projects/{id}/versions/{vid}/rollback` | 创建回滚草稿 |
| `GET` | `/api/projects/{id}/versions/{vid}/review` | 查看版本差异和风险 |
| `POST` | `/api/projects/{id}/versions/{vid}/review-approval` | 确认版本审查 |

### 决策服务运行

| 方法 | 路径 | 用途 |
|------|------|------|
| `GET` | `/api/projects/{id}/decision-services` | 列出已发布服务 |
| `POST` | `/api/projects/{id}/decision-services/execute` | 调用决策服务（服务 ID 在请求体内） |
| `GET/POST` | `/api/projects/{id}/decision-service-keys` | 查看或创建对外调用凭证（需登录） |
| `DELETE` | `/api/projects/{id}/decision-service-keys/{kid}` | 撤销对外调用凭证（需登录） |
| `POST` | `/api/public/projects/{id}/decision-services/execute` | 外部系统免登录执行（需 `X-Decision-Key`；服务 ID 在请求体内） |
| `GET` | `/api/projects/{id}/decision-runs` | 查看运行历史 |
| `PUT` | `/api/projects/{id}/decision-runs/{rid}/outcome` | 回填业务结果 |
| `GET` | `/api/projects/{id}/decision-analytics` | 查看已发布版本分析 |
| `GET` | `/api/projects/{id}/optimization-proposals` | 查看优化建议 |
| `POST` | `/api/projects/{id}/optimization-proposals/generate` | 生成优化建议 |
| `POST` | `/api/projects/{id}/optimization-proposals/{pid}/confirm` | 确认建议并创建草稿 |
| `POST` | `/api/projects/{id}/optimization-proposals/{pid}/dismiss` | 驳回优化建议 |

### 对外调用已发布决策服务

发布版本后，先由拥有服务管理权限的成员创建仅限该服务使用的调用凭证。创建接口响应中的 `secret` 只会返回一次，应立即写入调用方的密钥管理系统；平台仅保存其哈希和前缀。`serviceId` 可从已发布服务列表中获取。

```bash
# 创建调用凭证（需要登录令牌）
curl -X POST http://127.0.0.1:3000/api/projects/<项目ID>/decision-service-keys \
  -H 'Authorization: Bearer <JWT>' \
  -H 'Content-Type: application/json' \
  -d '{
    "serviceId": "<服务ID>",
    "name": "order-service-production",
    "expiresAt": "2027-01-01T00:00:00Z"
  }'

# 以调用凭证执行服务；inputs 必须是 JSON 对象
curl -X POST http://127.0.0.1:3000/api/public/projects/<项目ID>/decision-services/execute \
  -H 'X-Decision-Key: dmn_live_<创建时返回的完整密钥>' \
  -H 'Content-Type: application/json' \
  -d '{
    "serviceId": "<服务ID>",
    "businessReference": "ORDER-2026-0001",
    "inputs": { "ApplicantAge": 32, "AnnualIncome": 120000 }
  }'
```

凭证仅可调用创建时指定的服务；被撤销、过期或格式不正确的凭证会被拒绝。单个项目最多保留 50 个有效凭证。

### 知识库管理

| 方法 | 路径 | 用途 |
|------|------|------|
| `GET/POST` | `/api/knowledge-sources` | 查看或创建知识源 |
| `PUT/DELETE` | `/api/knowledge-sources/{sid}` | 更新或删除知识源 |
| `GET` | `/api/projects/{id}/knowledge-sources` | 查看项目绑定的知识源 |
| `POST/DELETE` | `/api/projects/{id}/knowledge-sources/{sid}` | 绑定或解绑知识源 |

### 模型库

| 方法 | 路径 | 用途 |
|------|------|------|
| `GET` | `/api/model-library` | 查看模型库 |
| `POST` | `/api/model-library/{kind}/{sid}/instantiate` | 复用模板或历史模型 |
| `POST` | `/api/projects/{id}/versions/{vid}/templates` | 保存为模板 |
| `DELETE` | `/api/model-library/templates/{tid}` | 删除个人模板 |

> **注意**：`/api/run` 在客户端请求 `Accept: text/event-stream` 时会以 SSE 返回 `step`、`delta`、`result` 或 `error` 事件。

## 生产构建与部署

### 构建

使用仓库脚本构建前端静态资源与后端二进制：

```bash
./script/build-release.sh
```

构建产物：

- 后端可执行文件：`target/release/fuxi-dmn`
- 前端静态资源：`target/site/`（包含 WASM、JS 和样式表）

### Docker Compose 部署

仓库中的 Compose 配置会启动 PostgreSQL、Fuxi-DMN 和 Nginx。首次部署时，从生产模板创建本地配置并替换所有占位符：

```bash
cp .env.production.example .env
$EDITOR .env

# 先检查变量是否完整（不会启动服务）
docker compose --env-file .env config

# 构建并启动；应用端口为 3000，Nginx 端口为 80
docker compose --env-file .env up -d --build
curl http://127.0.0.1:3000/api/health
docker compose --env-file .env ps
```

`DB_PASSWORD`、`JWT_SECRET` 和 `ADMIN_PASSWORD` 是必需配置；`OPENAI_API_KEY` 仅在使用智能建模时需要。数据库初始化脚本只会在首次创建 `postgres_data` 卷时自动执行，已有数据库升级请按上面的增量 SQL 顺序执行。停止服务时使用 `docker compose --env-file .env down`；不要使用 `down -v`，除非确认要删除数据库卷。

如果 Docker Hub 不可访问，可使用能访问的 Rust 1.88 镜像构建：

```bash
RUST_IMAGE=your-registry/rust:1.88-bookworm \
  docker compose --env-file .env build
docker compose --env-file .env up -d
```

也可以使用 `docker-compose.prebuilt.yml` 启动已经导入本机的 `fuxi-dmn:latest` 镜像：

```bash
docker compose -f docker-compose.prebuilt.yml --env-file .env up -d
```

### 本地编译后部署到腾讯云轻量应用服务器

腾讯云轻量应用服务器通常运行 Ubuntu/Debian Linux。若本地是 macOS 或
Windows，不能直接上传本机执行 `cargo build --release` 生成的二进制，必须先
按服务器架构编译 Linux 版本。先确认服务器架构：

```bash
ssh ubuntu@<服务器公网IP> 'uname -m'
```

`x86_64` 使用 `linux/amd64`，`aarch64` 使用 `linux/arm64`。以下示例以
`x86_64` 为例，在项目根目录执行（本地需要 Docker Desktop）：

```bash
# 使用 Dockerfile 的 builder 阶段编译 Linux 后端、WASM 和静态资源
docker buildx build \
  --platform linux/amd64 \
  --target builder \
  --load \
  -t fuxi-dmn-builder:release .
```

Dockerfile 默认从 Docker Hub 使用 `rust:1.88-bookworm`。如果当前网络无法访问 Docker Hub，
可将 `RUST_IMAGE` 替换为组织或云厂商提供的、可访问的 Rust 1.88 镜像：

```bash
docker buildx build --build-arg RUST_IMAGE=your-registry/rust:1.88-bookworm \
  --platform linux/amd64 --target builder --load \
  -t fuxi-dmn-builder:release .
```

`build-and-upload-image.sh` 和 `docker compose build` 同样支持这个环境变量。

### 从构建镜像中提取部署产物
```bash
cid=$(docker create fuxi-dmn-builder:release)
rm -rf dist
mkdir -p dist/target/release
docker cp "${cid}:/app/target/release/fuxi-dmn" \
  dist/target/release/fuxi-dmn
docker cp "${cid}:/app/target/site" dist/target/site
docker rm "$cid"

# 打包；不要把生产密钥放进这个压缩包
tar -C dist -czf fuxi-dmn-release-linux-amd64.tar.gz .
file dist/target/release/fuxi-dmn
```

ARM 服务器只需将 `--platform linux/amd64` 改为 `--platform linux/arm64`，并相应
修改压缩包文件名。

上传并安装（首次部署）：

```bash
scp fuxi-dmn-release-linux-amd64.tar.gz ubuntu@<服务器公网IP>:/tmp/
scp script/init.sql ubuntu@<服务器公网IP>:/tmp/fuxi-init.sql
ssh ubuntu@<服务器公网IP>

sudo apt update
sudo apt install -y ca-certificates libssl3 postgresql postgresql-contrib
sudo useradd --system --home /opt/fuxi-dmn --shell /usr/sbin/nologin fuxi  # 已存在时跳过
sudo mkdir -p /opt/fuxi-dmn
sudo tar -xzf /tmp/fuxi-dmn-release-linux-amd64.tar.gz -C /opt/fuxi-dmn
```

创建数据库用户并初始化新数据库（已有用户或数据库时跳过对应创建步骤）：

```bash
sudo -u postgres psql -c "CREATE USER fuxi WITH PASSWORD '<数据库密码>';"
sudo -u postgres psql -v app_user=fuxi -f /tmp/fuxi-init.sql
```

创建 `/etc/fuxi-dmn.env`，至少配置以下变量：

```dotenv
DATABASE_URL=postgres://fuxi:<数据库密码>@127.0.0.1:5432/fuxi_office
ADMIN_PASSWORD=<管理员初始密码，至少6位>
JWT_SECRET=<至少32位随机字符串>
APP_ENV=production
BIND_ADDR=127.0.0.1:3000
OPENAI_API_KEY=<兼容接口密钥>
OPENAI_BASE_URL=https://api.openai.com/v1
OPENAI_MODEL=gpt-4-turbo
RUST_LOG=info
```

然后创建 `/etc/systemd/system/fuxi-dmn.service`：

```ini
[Unit]
Description=Fuxi DMN
After=network.target postgresql.service

[Service]
User=fuxi
WorkingDirectory=/opt/fuxi-dmn
EnvironmentFile=/etc/fuxi-dmn.env
ExecStart=/opt/fuxi-dmn/target/release/fuxi-dmn
Restart=always
RestartSec=5
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
```

启动并检查：

```bash
sudo chown -R fuxi:fuxi /opt/fuxi-dmn
sudo chmod 600 /etc/fuxi-dmn.env
sudo systemctl daemon-reload
sudo systemctl enable --now fuxi-dmn
sudo systemctl status fuxi-dmn
curl http://127.0.0.1:3000/api/health
```

应用默认监听 `127.0.0.1:3000`，生产环境建议用 Nginx/Caddy 反向代理到该端口并
配置 HTTPS；若直接对公网提供服务，将 `BIND_ADDR` 改为 `0.0.0.0:3000`，并在
腾讯云防火墙放行 `3000` 端口。后续更新时重新生成压缩包并上传，再执行：

```bash
sudo systemctl stop fuxi-dmn
sudo tar -xzf /tmp/fuxi-dmn-release-linux-amd64.tar.gz -C /opt/fuxi-dmn
sudo chown -R fuxi:fuxi /opt/fuxi-dmn
sudo systemctl start fuxi-dmn
```

### 运行

手工运行时，在当前工作目录放置 `.env`，或先导出同名环境变量：

```bash
./target/release/fuxi-dmn
```

使用上面的 systemd 配置时，环境变量由 `/etc/fuxi-dmn.env` 的
`EnvironmentFile` 注入，不需要将生产密钥打包到构建产物中。

后端会同时提供 API、`/pkg` 前端资源与单页应用入口；直接访问 `/login` 也能正常加载登录页。

### 部署选项

1. **集成部署**：单一二进制同时提供 API 和静态资源
   - 适合小规模部署和开发环境
   - 配置 `BIND_ADDR` 指定监听地址

2. **分离部署**：
   - 后端：运行 `target/release/fuxi-dmn` 作为 API 服务
   - 前端：将 `target/site/` 部署到静态托管（Nginx、CDN）
   - 前端请求默认使用浏览器当前 origin；应通过反向代理将 `/api` 转发到后端，或同步调整前端 API 地址实现

3. **容器化部署**：
   - 参见上面的 [Docker Compose 部署](#docker-compose-部署)
   - 确保 `DB_PASSWORD`、`JWT_SECRET` 和 `ADMIN_PASSWORD` 通过环境变量注入
   - 生产环境不要把 `.env`、API Key 或数据库密码打包进镜像和发布压缩包

## 测试

运行完整测试套件：

```bash
cargo test --lib
```

测试覆盖：
- **DMN 引擎**：DMN XML 解析、编译和运行时测试
- **FEEL 解释器**：算术、逻辑、字符串、日期时间、列表、上下文操作
- **DSL 校验**：类型引用、依赖循环、名称唯一性
- **DMN 编译**：DSL → DMN XML 转换正确性
- **运行时**：决策执行、命中策略、依赖解析

## 故障排查

### 常见问题

**1. 数据库连接失败**
```bash
错误：连接 PostgreSQL 失败
解决：
- 确认 PostgreSQL 已启动
- 检查 DATABASE_URL 配置正确
- 验证数据库和用户已创建（script/init.sql）
```

**2. 权限拒绝错误**
```bash
错误：permission denied for table users
解决：使用管理员账号执行授权 SQL（见"权限问题修复"部分）
```

**3. LLM 生成失败**
```bash
错误：模型生成超时或返回空结果
解决：
- 检查 OPENAI_API_KEY 和 OPENAI_BASE_URL 配置
- 验证 API 可访问性（curl 测试）
- 查看服务端日志中的详细错误
- 尝试降低模型复杂度或调整 temperature
```

**4. WASM 编译失败**
```bash
错误：error: failed to build target/site/pkg/...
解决：
- 确认已安装 wasm32-unknown-unknown target
- 重新安装项目要求的版本：`cargo install cargo-leptos --version 0.3.7 --locked`
- 清理构建缓存: cargo clean
```

**5. 前端 404 错误**
```bash
错误：GET /pkg/fuxi-dmn.js 404
解决：
- 确认 cargo leptos watch/build 成功完成
- 检查 target/site/pkg/ 目录存在
- 验证 BIND_ADDR 和实际访问地址一致
```

**6. Docker 基础镜像无法拉取**
```text
错误：failed to resolve source metadata for rust:1.88-bookworm
解决：
- 确认 Docker Desktop 或服务器 Docker daemon 正在运行
- 为 Docker 配置可访问的镜像仓库，并通过 RUST_IMAGE 覆盖基础镜像：
  RUST_IMAGE=your-registry/rust:1.88-bookworm docker compose --env-file .env build
- 使用 build-and-upload-image.sh 时同样设置 RUST_IMAGE 环境变量
```

## 项目组成

- **核心模块**：
  - DMN 引擎、FEEL 解释器和 NL-to-DSL 智能体
  - 前端组件
  - 后端 API 与业务逻辑
- **数据库表**：初始化脚本覆盖用户、项目、版本、测试、服务、知识库、审计和模型变更等核心数据

## 贡献与支持

### 文档

- [用户操作手册](docs/user-operation-manual.md) - 完整的业务操作流程和门禁说明
- [知识库示例](docs/knowledge-base-examples.md) - 知识库录入和建模关联
- [平台设计方案](docs/intelligent-decision-platform-design.md) - 整体架构和设计理念
- [贡献指南](CONTRIBUTING.md) - 开发环境、提交和测试要求
- [安全策略](SECURITY.md) - 漏洞报告和敏感信息处理
- [行为准则](CODE_OF_CONDUCT.md)
- [变更日志](CHANGELOG.md)

### 技术栈版本

- Rust 1.88 / Rust 2024 edition
- Axum 0.8
- Leptos 0.8.20
- PostgreSQL 14+
- SQLx 0.8
- Quick-xml 0.36

### 许可证

本项目自有代码以 MIT License 发布，详见 [LICENSE](LICENSE)。
`resource/` 下的第三方 DMN 示例可能适用其文件中声明的独立许可证，详见 [NOTICE](NOTICE)。

---

**伏羲智能决策工作台** - 让业务规则建模像对话一样简单
