# 知识库与建模关联示例

本文提供一组可以直接录入“资源 > 知识库”，并用于“建模”页面的示例。示例中的名称、版本和日期可以按实际企业制度替换。

## 1. 知识库示例

### 1.1 企业术语：高风险客户

页面录入：

| 字段 | 示例值 |
| --- | --- |
| 类型 | 企业术语 |
| 名称 | 高风险客户 |
| 版本 | 2026.1 |
| 说明 | 统一风险管理、授信和贷后系统中的客户风险分层口径 |
| 来源地址 | `https://intranet.example.com/glossary/customer-risk` |
| 生效日期 | `2026-01-01` |
| 失效日期 | 留空 |

知识内容：

```text
高风险客户（High Risk Customer）是指满足以下任一条件的客户：
1. 外部风险评分小于 600；
2. 最近 12 个月存在逾期 90 天以上记录；
3. 客户或实际控制人命中内部制裁名单。

风险等级取值：LOW、MEDIUM、HIGH。
本术语不决定是否拒绝业务，只规定风险分层口径。
```

### 1.2 字段目录：外部风险评分

页面录入：

| 字段 | 示例值 |
| --- | --- |
| 类型 | 字段目录 |
| 名称 | 外部风险评分 |
| 版本 | score-v3 |
| 说明 | 授信申请使用的外部风险评分字段定义 |
| 来源地址 | `https://data.example.com/catalog/risk_score` |
| 生效日期 | `2026-01-01` |

知识内容：

```text
字段名：externalRiskScore
中文名：外部风险评分
类型：number
范围：[300, 900]
单位：无
精度：整数
是否必填：是
数据来源：外部征信服务 risk-score-v3
空值处理：缺失时不得自动按 0 处理，应转人工审核。
```

### 1.2.1 一个字段目录包含多个字段

字段目录支持多个字段。当前实现建议按业务域建立一个知识源，在“知识内容”中用固定模板逐项描述字段：

```text
字段目录：授信申请基础字段
版本：credit-application-v2

字段 1
字段名：age
中文名：申请人年龄
类型：number
范围：[18, 75]
单位：岁
是否必填：是
空值处理：缺失时进入人工审核

字段 2
字段名：monthlyIncome
中文名：月收入
类型：number
范围：[0, 10000000]
单位：CNY/月
是否必填：是
空值处理：缺失时进入人工审核

字段 3
字段名：externalRiskScore
中文名：外部风险评分
类型：number
范围：[300, 900]
单位：无
是否必填：是
数据来源：外部征信服务 risk-score-v3
空值处理：缺失时不得按 0 处理

字段 4
字段名：customerSegment
中文名：客户分层
类型：string
允许值：["LOW", "MEDIUM", "HIGH"]
是否必填：否
```

在建模需求中可以一次引用这个目录中的多个字段：

```text
请使用已绑定的“授信申请基础字段”字段目录。模型输入使用 age、monthlyIncome、externalRiskScore 和 customerSegment，类型、范围、单位、必填规则和空值处理以字段目录为准。
```

如果字段需要独立负责人、不同生效日期或不同权限，建议拆成多个知识源，例如“外部风险评分字段”“客户分层字段”，分别绑定和维护。拆分不会改变 DSL 的输入定义，DSL 仍只需要在 `inputs` 中声明实际使用的字段。

### 1.3 制度政策：授信审批政策

页面录入：

| 字段 | 示例值 |
| --- | --- |
| 类型 | 制度政策 |
| 名称 | 授信审批政策 |
| 版本 | 2026-Q1 |
| 说明 | 个人贷款授信审批和人工复核要求 |
| 来源地址 | `https://intranet.example.com/policy/credit-approval-2026-q1` |
| 生效日期 | `2026-01-01` |
| 失效日期 | `2026-03-31` |

知识内容：

```text
个人贷款审批规则：
1. 年龄小于 18 岁，不得批准。
2. 外部风险评分小于 600，不得批准。
3. 年龄大于等于 18 岁、评分大于等于 750 且月收入大于等于 50000 时，可自动批准。
4. 其他情况必须进入人工审核。
5. 输入字段缺失、评分超出 [300, 900] 或来源不可用时，不得自动批准。

审批输出只能是：Approve、Reject、Manual Review。
```

## 2. 使用 API 创建知识源

登录后调用 `POST /api/knowledge-sources`。`source_type` 只能是 `term`、`field` 或 `policy`，日期使用 `YYYY-MM-DD`。

```json
{
  "source_type": "policy",
  "name": "授信审批政策",
  "description": "个人贷款授信审批和人工复核要求",
  "content": "个人贷款审批规则：\n1. 年龄小于 18 岁，不得批准。\n2. 外部风险评分小于 600，不得批准。\n3. 年龄大于等于 18 岁、评分大于等于 750 且月收入大于等于 50000 时，可自动批准。\n4. 其他情况必须进入人工审核。",
  "version_label": "2026-Q1",
  "source_uri": "https://intranet.example.com/policy/credit-approval-2026-q1",
  "valid_from": "2026-01-01",
  "valid_until": "2026-03-31",
  "status": "active"
}
```

创建后，在目标项目的知识目录中勾选“绑定当前项目”。只有“当前用户拥有、已绑定、启用且处于有效期内”的知识正文会注入智能建模。创建版本时服务端会重新校验绑定状态并保存引用快照。

## 3. 建模关联知识库：自然语言示例

先绑定“高风险客户”“外部风险评分”“授信审批政策”三个知识源，然后在“建模 > 新建模型”输入：

```text
创建个人贷款授信审批模型。

输入：年龄 age（number）、月收入 monthlyIncome（number）、外部风险评分 externalRiskScore（number）。
输出：Approval，取值只能是 Approve、Reject、Manual Review。

请使用当前项目已绑定的“授信审批政策”作为审批规则来源，使用“外部风险评分”作为字段定义，使用“高风险客户”作为风险术语说明。
规则：年龄小于 18 岁或评分小于 600 时 Reject；年龄至少 18 岁、评分至少 750 且月收入至少 50000 时 Approve；其他情况 Manual Review。
缺失输入或评分不在 [300, 900] 时不得自动批准。
请在模型中保留法规/制度的知识源引用，并让 Approval 声明对该知识源的权威依赖。
```

生成模型后，在模型页顶部的“当前版本知识依据”可以看到本次版本实际使用的知识引用。若修改知识源、解除绑定或知识过期，需要重新生成后才能保存新版本。

## 4. 建模关联知识库：DSL 示例

下面是与上述知识源对应的关键 DSL 片段。`knowledgeSources` 是模型产物中的权威来源声明，`authorityTargets` 指向具体决策名称；它不替代项目绑定操作。

```yaml
name: LoanApprovalWithPolicy
namespace: https://example.com/dmn/loan-approval-with-policy
description: 使用授信审批政策和风险字段目录的个人贷款审批模型
inputs:
  - name: age
    typeRef: number
  - name: monthlyIncome
    typeRef: number
  - name: externalRiskScore
    typeRef: number
decisions:
  - name: Approval
    typeRef: string
    requiredInputs: [age, monthlyIncome, externalRiskScore]
    requiredDecisions: []
    requiredKnowledge: []
    logic:
      type: decisionTable
      hitPolicy: FIRST
      inputs:
        - name: age
          expression: age
          typeRef: number
        - name: externalRiskScore
          expression: externalRiskScore
          typeRef: number
        - name: monthlyIncome
          expression: monthlyIncome
          typeRef: number
      outputs:
        - name: Approval
          typeRef: string
          defaultOutput: '"Manual Review"'
      rules:
        - inputEntries: ['< 18', '-', '-']
          outputEntries: ['"Reject"']
          annotation: 未成年不得批准
        - inputEntries: ['-', '< 600', '-']
          outputEntries: ['"Reject"']
          annotation: 风险评分过低不得批准
        - inputEntries: ['>= 18', '>= 750', '>= 50000']
          outputEntries: ['"Approve"']
          annotation: 满足自动批准条件
        - inputEntries: ['-', '-', '-']
          outputEntries: ['"Manual Review"']
          annotation: 其他情况人工审核
knowledgeSources:
  - name: 授信审批政策
    description: 个人贷款授信审批和人工复核要求
    url: https://intranet.example.com/policy/credit-approval-2026-q1
    authorityTargets: [Approval]
  - name: 外部风险评分
    description: externalRiskScore 字段定义与取值范围
    url: https://data.example.com/catalog/risk_score
    authorityTargets: [Approval]
```

### 4.1 关联 BKM 的场景

当规则需要复用一段知识逻辑时，使用 BKM，并在决策的 `requiredKnowledge` 中引用 BKM 名称。BKM 是 DMN 内的可执行业务知识模型，不是资源中心知识源的替代品。

```yaml
bkms:
  - name: IsHighRiskCustomer
    description: 根据风险评分判断客户是否属于高风险客户
    parameters:
      - name: score
        typeRef: number
    logic:
      type: functionDefinition
      parameters:
        - name: score
          typeRef: number
      body:
        type: literalExpression
        expression: score < 600
decisions:
  - name: RiskGate
    typeRef: boolean
    requiredInputs: [externalRiskScore]
    requiredDecisions: []
    requiredKnowledge: [IsHighRiskCustomer]
    logic:
      type: invocation
      bkm: IsHighRiskCustomer
      bindings:
        - parameter: score
          expression: externalRiskScore
```

## 5. 推荐的验收案例

绑定知识并生成模型后，在“验收”中添加以下案例：

| 案例 | 输入 JSON | 期望输出 JSON |
| --- | --- | --- |
| 未成年拒绝 | `{ "age": 17, "externalRiskScore": 800, "monthlyIncome": 80000 }` | `{ "Approval": "Reject" }` |
| 高评分自动批准 | `{ "age": 35, "externalRiskScore": 780, "monthlyIncome": 60000 }` | `{ "Approval": "Approve" }` |
| 低评分拒绝 | `{ "age": 35, "externalRiskScore": 580, "monthlyIncome": 60000 }` | `{ "Approval": "Reject" }` |
| 普通情况人工审核 | `{ "age": 35, "externalRiskScore": 680, "monthlyIncome": 60000 }` | `{ "Approval": "Manual Review" }` |

运行全部案例并批准通过结果后，再执行自动验收和发布。知识源后续变更不会修改已保存版本的知识引用快照。
