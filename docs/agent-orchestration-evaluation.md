# Agent 编排运行验收观测

`script/eval-agent-orchestrations.py` 读取已有的项目编排历史，输出聚合报告。它不执行模型调用、不修改业务数据，也不生成验收样本。服务端会按项目查看权限检查访问。

```bash
API_TOKEN='<project-view-token>' python3 script/eval-agent-orchestrations.py 7 \
  --api-base https://workbench.example \
  --token-budget 250000 \
  --minimum-cases 100 \
  --output target/orchestration-evaluation.json
```

`--token-budget` 是本次观测统一采用的正整数 Token 门槛。历史 API 不返回每次运行生效的预算配置，因此该选项不证明部署时确实启用了相同配置；应在报告之外保存观测窗口、提交、模型路由和预算配置等基线材料。未指定时预算验收状态为 `not_configured`，不默认采用任何环境变量值。

`--minimum-cases` 默认 100，可在验收计划中预先约定。它分别作用于编排记录、重试家族和预算树的有效分母；默认值是工具的样本门槛，不是方案已经核准的抽样设计。输出文件使用 `0600` 权限并原子替换，只包含汇总；原始错误、业务输入/输出、产物正文、编排 ID 不进入报告。模型名、任务种类仍作为统计分组键输出。

## 统计口径

| 报告字段 | 统计单位 | 成功分子与分母 |
|---|---|---|
| `orchestrationAttempts` | 一条持久编排记录，包括每次重试新建的记录 | `succeeded / (succeeded + failed)`；目标 95% |
| `retryFamilies.firstAttempt` | 只通过 `retryOfOrchestrationId` 归组后的最早记录 | 首次记录的 `succeeded / (succeeded + failed)` |
| `retryFamilies.latestAttempt` | 同一重试家族的最近记录 | 最近记录的 `succeeded / (succeeded + failed)` |
| `budgetTrees.completedWithinBudget` | `parentOrchestrationId` 与重试边共同连接的预算树 | 成功结束且累计 Token ≤ 门槛的树数 / 用量完整的有效终态树数；目标 90% |
| `roleFlow.coreRoleCompletion` | 同一父子/重试树内包含 Analyst、Architect 或 Engineer 的流程树 | 四个核心角色均至少有一个成功步骤的树数 / 入选流程树数；不设自动达标目标 |

编排成功率读取编排自身的 `status`，不由成功步骤的占比代替。`pending`、`running` 不进入完成率分母，`cancelled` 单独统计；它们没有被当作成功或普通失败。`statusCounts`、`inFlightCount` 保留这些数量，便于发现只看成功率时隐藏的取消或积压。

父子编排表示不同的阶段，不因父子关系而合并成重试家族。重试家族以持久 ID 的先后顺序确定首次与最近尝试；最近状态并不表示未来不会再重试。旧的步骤/模型首轮与最终尝试统计仍保留在 `taskKinds`、`models` 中，新增统计不改变这些字段的含义。

`roleFlow` 从步骤的 `agentRole` 汇总 Analyst、Architect、Engineer 和 Tester 的尝试/成功树数；只有含 Analyst、Architect 或 Engineer 的树进入流程分母，独立 Tester Judge 等树不会被误计为全链路。Facilitator 作为条件角色单列，不要求每条流程都经过澄清。角色步骤完成不等于业务产物正确，也不替代成功率、预算或业务验收。

## 预算树与用量覆盖

服务端的 `OPENAI_MAX_ORCHESTRATION_OBSERVED_TOKENS` 对整棵父子/重试树内 `agent_runs` 已记录的累计 Token 进行调用间门控。它不限制供应商正在执行的请求，多次在途调用也可能使实际观测总量越过门槛。评估器因此按整个预算树衡量最终结果，而非对每个重试重新给一份预算。

历史 API 只返回步骤摘要，不返回 `agent_run_id`。当前持久化约定是：每条编排的主步骤 `stepNo=1` 保存该 Agent run 的累计 `usage`；规划过程另记 Architect、Validator、Tester 等步骤，其中可以没有独立用量。工具每条编排只读取唯一主步骤的累计用量，然后求整树所有尝试的总和。它不会把附加步骤的累计快照再相加，也不会把这些步骤没有 `usage` 误认为一次模型调用丢失。

以下情况使该树用量未知：

- 某次编排缺少主步骤，或出现多个主步骤而无法可靠判断如何去重。
- 主步骤仍处于 `pending` / `running`。
- 主步骤没有用量，或用量既无 `total_tokens`，也没有同时提供 `prompt_tokens` 与 `completion_tokens`。
- 用量仍带 `budget_reservation: true`，表示预留记录尚未被实际观测替换。

明确记录的 `total_tokens: 0` 是零观测值；缺字段或缺记录是未知，二者不同。仅在输入/输出两个分量都存在时才补算 `total_tokens`；布尔值、负数、溢出和小于两个分量之和的总量都会被拒绝。服务端自身可能将供应商未报告的用量保存为零，历史 API 无法证明这种零值等于供应商实际使用量或计费用量，因此报告只衡量持久化观测，不代表费用核对完成。

树内有任意活跃尝试时，该树记为 `inFlight`。所有尝试均已终止后，先取每个重试家族的最近状态；其中有取消则该树记为 `cancelled` 并单列，否则有失败则记为 `failed`，其余记为 `succeeded`。已失败后被成功重试恢复的旧尝试仍消耗预算，但不再单独使家族失败。

`evaluableTrees` 为成功或失败的闭合树数。`tokenObservedTrees`、`tokenMissingTrees` 与 `tokenCoverage` 展示其中的用量完整程度。`completedWithinBudget` 的数值仅基于用量完整树；`completedWithinBudgetLowerBound` 同时以全部有效终态树为分母，将未知暂按未证实成功计算保守下界。任何未知用量都会阻止预算验收返回 `target_met`，即使用量完整子集显示 100%。

## 状态与区间

目标状态为：

- `not_configured`：没有显式指定预算门槛，只适用于预算指标。
- `insufficient_samples`：有效分母小于预先设置的样本门槛。
- `incomplete_usage`：样本数已足够，但预算用量覆盖不完整。
- `target_met` / `target_not_met`：样本和用量条件满足后，根据观测点估计是否达到 95% / 90% 目标判定。

成功率、用量覆盖率及预算完成率另给出 Wilson 95% 区间；空分母返回 `null`。达标判定采用点估计，不要求区间下界也达到目标。区间没有校正重试之间、同一业务或同一项目之间的相关性，也不能消除样本选择偏差。

## 范围与限制

工具分页读取全部可见项目历史，最多 10,000 条编排、100,000 条步骤；单页最多 100 条。它不是指定生产窗口的固定快照，分页期间仍可能有任务写入或变化。调用者应选择稳定的验收窗口并保留范围证据；如果预算配置、模型、路由或任务分布变化，应先分批观测，不能把混合样本解释为受控比较。

若历史缺少所引用的祖先或重试源，工具拒绝给出结果，避免把截断谱系当作独立预算。父子/重试环与指向多个独立预算根的歧义关系同样被拒绝。谱系使用迭代拓扑遍历，长链不依赖 Python 递归深度。步骤尝试号既兼容每个新编排从 1 重新计数，也兼容当前工作流持久化的跨编排延续编号；单条编排内部仍要求连续。

若后续 API 改变主步骤累计用量约定，或同一编排开始关联多个独立 Agent run，应同步扩展 API 以提供可去重的运行标识及其用量，再调整评估器；当前字段不足以还原任意多 Run 布局。该工具补齐观测计算路径，不证明真实编排成功率、预算完成率或多 Agent 业务验收已经达标。
