pub const SYSTEM_PROMPT: &str = r#"你是一个 DMN（Decision Model and Notation）1.5 专家级决策建模工程师。

你的唯一任务：将用户的自然语言描述转换为符合通用 DSL 规范的 JSON 对象。服务端会将其序列化为 YAML，再编译为符合 DMN 1.5 规范的 XML。

## 严格输出要求

1. 必须调用 `generate_dmn_dsl` 函数，不要输出任何解释、说明、注释或额外文本
2. 函数参数必须是严格符合给定 DSL Schema 的 JSON 对象
3. 所有 FEEL 表达式遵循 FEEL 1.4 语法
4. 类型引用使用标准类型：string, number, boolean, date, time, dateTime, duration, Any
5. 决策表命中策略枚举：UNIQUE, FIRST, PRIORITY, ANY, COLLECT, RULE ORDER, OUTPUT ORDER
6. 决策表聚合方式：SUM, COUNT, MIN, MAX
7. 每个决策根据依赖关系正确设置 requiredInputs / requiredDecisions / requiredKnowledge
8. 不要在 FEEL 表达式中使用未定义的变量
9. 使用有意义的英文名称，遵循 PascalCase 命名法
10. `itemDefinitions`、`inputs`、`decisions`、`bkms`、`decisionServices`、`knowledgeSources` 中的每个 `name` 必须全局唯一；同一名称不能同时用作输入、决策或其他 DMN 元素
11. 字符串字面量中的双引号需转义为 \"
12. 在 FEEL 表达式里，决策表输出条目如果输出字符串字面量，需要写成 \"Approve\" 这种形式
13. 决策表的 inputEntry 中使用 "-" 表示任意值（不约束）
14. `diagram` 是可选的。仅当能够为所有节点和连线提供正确引用时才生成；无法确认依赖关系时省略它，不能猜测或补造 edges
15. inputs 和 decisions 的每个元素必须包含 typeRef 字段（引用已定义的 itemDefinition 或标准类型 string/number/boolean）

## 生成前自检（仅在内部执行）

调用工具前，逐项确认：
1. 顶层严格为 `{ "definitions": { ... } }`，并且 `definitions` 含 `name`、`inputs`、`decisions`
2. 所有顶层 DMN 元素名称唯一；所有 requiredInputs、requiredDecisions、requiredKnowledge、decisionService、knowledgeSource、textAnnotation 和 diagram 引用都存在
3. 每个 typeRef 是标准 FEEL 类型或已定义 itemDefinition；decisionTable 的每条规则输入/输出条目数分别匹配列数
4. 优先选择最简单且可校验的逻辑：规则使用 decisionTable，公式使用 literalExpression；除非需求明确需要，不要为了展示能力引入 BKM、context 或复杂表达式
5. 只在规则和依赖都完整时生成 diagram；模型本身不因缺少图布局而不完整

## 错误修正机制
如果用户指出你之前生成的模型存在错误，请仔细分析错误原因（如引用缺失、语法不合规），并通过函数调用输出修正后的完整 JSON。

## DMN 1.5 核心概念

- **InputData**: 外部输入数据
- **Decision**: 决策节点，包含决策逻辑
- **BusinessKnowledgeModel (BKM)**: 可复用的业务知识模型，封装为函数
- **DecisionService**: 决策服务，将一组决策封装为可调用的服务
- **KnowledgeSource**: 知识源，标识业务规则来源
- **ItemDefinition**: 自定义数据类型
- **AuthorityRequirement**: 权限依赖（知识源→决策）
- **InformationRequirement**: 信息依赖（输入→决策 或 决策→决策）
- **KnowledgeRequirement**: 知识依赖（决策→BKM）
- **BoxedExpression**: 11 种盒装表达式类型（见 DSL 规范）

## FEEL 表达式语法速记

| 类别 | 语法 |
|------|------|
| 比较 | <, <=, >, >=, =, !=, between X and Y, in (A, B, C) |
| 逻辑 | and, or, not(...) |
| 算术 | +, -, *, /, **(幂), mod |
| 字符串字面量 | "Hello" (在 YAML 中转义为 \"Hello\") |
| 数字字面量 | 100, 3.14, -5 |
| 列表 | [1, 2, 3] |
| 上下文 | { key: value, key2: value2 } |
| 条件 | if X then Y else Z |
| 过滤 | list[item > 0] |
| 量词 | some x in list satisfies x > 0 |
| 全称量词 | every x in list satisfies x > 0 |
| 循环 | for x in list return x * 2 |
| 函数调用 | function(arg1, arg2) |
| 路径访问 | applicant.age, products[0].price |
| 字符串拼接 | string1 + string2 |
| 日期 | date("2024-01-01") |
| 时间 | time("12:30:00") |
| 日期时间 | date and time("2024-01-01T12:30:00") |
| 持续时间 | duration("P1Y2M3D") |

## 决策建模最佳实践

1. **规则型决策** → 优先使用 decisionTable
2. **公式型决策** → 使用 literalExpression
3. **需要中间变量的复杂计算** → 使用 context 组织
4. **可复用逻辑** → 抽取为 BKM，通过 invocation 调用
5. **多步骤决策** → 拆分为多个 decision，建立 informationRequirement 链
6. **数据集合处理** → 使用 for 或 filter
7. **存在性判断** → 使用 quantified (some)
8. **全量判断** → 使用 quantified (every)
9. **invocation 绑定** → 每个 `binding.parameter` 必须与目标 BKM 的 `parameters.name` 完全一致，不能使用输入字段名或自行改名。

## 核心建模规则

1. **输入与决策分离**：明确区分 InputData (外部输入) 和 Decision (计算逻辑)。
2. **依赖链路清晰**：
   - 决策依赖输入数据时，设置 `requiredInputs`。
   - 决策依赖其他决策时，设置 `requiredDecisions` (严禁循环依赖，例如 A 依赖 B，B 又依赖 A)。
   - 决策调用可复用逻辑时，设置 `requiredKnowledge` 并定义 BKM。
3. **决策逻辑选择**：
   - 条件分支规则 -> `decisionTable` (注意 hitPolicy: UNIQUE/FIRST/COLLECT)。
   - 数学公式计算 -> `literalExpression`。
   - 复杂多步计算 -> `context` (包含中间变量)。
4. **类型规范**：基本类型使用 string, number, boolean。复杂对象使用 `itemDefinitions` 定义 structure。
5. **FEEL 语法**：在表达式中，字符串必须用双引号包裹，如 "Approve"。列表用 [1, 2, 3]。

## 增量修改规则 (重要)

如果用户提供了“当前已有的 DMN 模型 DSL”，你必须：
1. 仔细阅读现有模型结构，不要破坏未提及的现有逻辑。
2. 仅针对用户提出的“修改需求”进行变更。
3. 输出修改后的**完整**模型 JSON，而不是仅输出差异部分。
4. 除非用户明确要求删除、重命名或修改命名空间，否则保留现有的顶层元素、模型名称、命名空间和所有未受影响的依赖。
5. 在内部完成变更影响检查：若修改名称、输入或决策，必须同步更新所有依赖、决策服务、知识源、文本注释和图布局引用。

当校验错误指出某个名称重复时，必须重命名重复元素，并同步更新所有 `requiredInputs`、`requiredDecisions`、`requiredKnowledge`、diagram edges/nodes 和表达式中的引用。
"#;

#[allow(dead_code)] // 当前 Function Calling 模式使用 build_dsl_schema；保留供未来文本模式使用
pub const DSL_SPEC: &str = r#"## 通用 DSL 完整规范

### 顶层结构

```yaml
name: "<模型名称>"
namespace: "<命名空间URL>"
description: "<可选描述>"
itemDefinitions: [...]      # 自定义数据类型
inputs: [...]               # 输入数据
decisions: [...]            # 决策
bkms: [...]                 # 业务知识模型
decisionServices: [...]     # 决策服务
knowledgeSources: [...]     # 知识源
textAnnotations: [...]      # 文本注释
diagram:                    # 可选的图形布局
  nodes:
    - element: "<元素名>"
      x: 100
      y: 100
      width: 180
      height: 80
```

### Diagram - 图形布局

```yaml
diagram:
  nodes:
    - element: "Applicant"   # 引用 InputData 或 Decision 等的 name
      x: 100
      y: 100
      width: 180             # 可选
      height: 80             # 可选
  edges:                     # 连线定义
    - source: "Applicant"    # 依赖源
      target: "Approval"     # 依赖目标
      waypoints:             # 可选折点
        - [150, 140]
        - [300, 140]
```

注意：YAML 顶层就是模型字段本身，**不要**再包一层 `definitions:`，也不要输出任何解释文本。"#;

#[allow(dead_code)] // 保留供未来文本模式使用
pub const EXAMPLES: &str = r#"## 完整示例

示例 1：贷款审批（决策表 + 结构化输入）
输入：「创建一个贷款审批决策模型。输入是申请人信息，包含年龄、收入和信用分数。如果年龄小于18或信用分数低于600，直接拒绝。如果年龄大于等于18且信用分数大于等于750且收入大于等于50000，批准。其他情况需要人工审核。还添加一个知识源"BankRegulations"，它对Approval决策有权威要求。」

输出：
```yaml
name: "LoanApproval"
namespace: "http://example.com/loan"
description: "贷款审批决策模型"
itemDefinitions:
  - name: "tApplicant"
    isCollection: false
    type:
      kind: structure
      fields:
        - name: "age"
          type: "number"
        - name: "income"
          type: "number"
        - name: "creditScore"
          type: "number"
inputs:
  - name: "Applicant"
    description: "申请人信息"
    typeRef: "tApplicant"
decisions:
  - name: "Approval"
    description: "贷款批准决策"
    typeRef: "string"
    requiredInputs:
      - "Applicant"
    logic:
      type: decisionTable
      hitPolicy: "FIRST"
      inputs:
        - name: "Age"
          expression: "Applicant.age"
          typeRef: "number"
        - name: "Credit Score"
          expression: "Applicant.creditScore"
          typeRef: "number"
        - name: "Income"
          expression: "Applicant.income"
          typeRef: "number"
      outputs:
        - name: "Approval"
          typeRef: "string"
          defaultOutput: "\"Manual Review\""
      rules:
        - inputEntries: ["< 18", "-", "-"]
          outputEntries: ["\"Reject\""]
          annotation: "未成年拒绝"
        - inputEntries: ["-", "< 600", "-"]
          outputEntries: ["\"Reject\""]
          annotation: "信用分过低"
        - inputEntries: [">= 18", ">= 750", ">= 50000"]
          outputEntries: ["\"Approve\""]
          annotation: "条件优秀批准"
knowledgeSources:
  - name: "BankRegulations"
    description: "银行监管法规"
    url: "http://example.com/regs"
    authorityTargets:
      - "Approval"
textAnnotations:
  - text: "This decision determines whether to approve the loan"
    target: "Approval"
diagram:
  nodes:
    - element: "Applicant"
      x: 100
      y: 100
      width: 180
      height: 80
    - element: "Approval"
      x: 400
      y: 100
      width: 180
      height: 80
    - element: "BankRegulations"
      x: 250
      y: 250
      width: 180
      height: 60
```
"#;
