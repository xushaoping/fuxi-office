# 偏差归因标签统计与 M3 验收边界

`script/eval-deviation-reviews.py` 统计已经人工标注的候选规则归因。现有项目标签可以改判，导出文件不含审阅者身份、独立性或盲审过程证据，所以工具始终输出 `m3Acceptance.status = not_established`、`passed = false`。数值达到目标不能据此认定 M3 已完成。

## 输入与使用

每个文件限一个项目。先导出人工标签，再导出包含未审阅案例的归因清单；使用匹配的两份文件运行：

```bash
API_TOKEN='<项目查看令牌>' python3 script/export-deviation-reviews.py <项目 ID> \
  --output target/deviation-reviews.jsonl
API_TOKEN='<项目查看令牌>' python3 script/export-deviation-inventory.py <项目 ID> \
  --output target/deviation-inventory.jsonl
python3 script/eval-deviation-reviews.py target/deviation-reviews.jsonl \
  --inventory target/deviation-inventory.jsonl \
  --output target/deviation-attribution-report.json
```

工具只读取本地文件，不访问数据库、业务服务或模型服务，也不生成标签。`--inventory` 可省略；省略后仅能报告已审阅样本，未审阅数和总体覆盖率返回 `null`，不能推断总体归因表现。空标签文件可以结合非空清单报告“全部尚未审阅”。

审阅 JSONL 每行必须恰好包含：

| 字段 | 口径 |
|---|---|
| `caseId` | 非空案例 ID，最长 255 字符，通常为 `deviation-<归因记录 ID>` |
| `decisionRunId` | 正的 64 位整数；同一文件中每次决策运行只能出现一次 |
| `status` | `candidate` 或 `insufficient_evidence` |
| `attributedRule` | `candidate` 必须有非空规则 ID；无证据记录可以为 `null` |
| `confidence` | 0–1 的有限数值，禁止布尔值 |
| `label` | `correct`、`incorrect` 或 `inconclusive`；无证据记录只能是不确定 |

归因清单每行恰好包含 `caseId`、`decisionRunId`、`status`、`confidence`，不含标签、规则 ID、业务输入/结果或证据正文。清单也要求唯一案例 ID 和唯一决策运行 ID。

清单来源是新增的只读 `GET /api/projects/{id}/deviation-analysis-inventory?afterId=0&limit=500`。接口校验项目查看权限，返回 `{throughId, records}`；首次请求在只读事务内确定项目最大归因 ID，后续传同一 `throughId` 固定上界，按 ID 正序读取。导出器连续遍历两次相同边界内的清单，内容变化时拒绝替换文件，成功时以 `0600` 权限原子写入。上界之后新增的案例不属于本次清单；两次遍历一致也不等于跨请求事务快照，不能恢复已删除历史或证明全部业务运行都已归因。

部署前在已有库应用 `098_deviation_analysis_inventory_index.sql`；新建库已包含 `(project_id, id)` 索引。原有 `(project_id, created_at, id)` 索引不直接支持 ID 顺序翻页。索引构建需表拥有者/管理员执行；生产规模的扫描和导出延迟仍需目标环境测量。

提供清单时，每条标签的上述四个公共字段必须与清单一致，未知案例和快照不一致都会拒绝。两个文件不是数据库事务快照；若导出间发生归因重算或置信度变动，应重新导出并核对。工具不能证明清单没有遗漏，也不能验证标签的业务正确性。

两个文件均允许空行和 `#` 注释；每行最多 64 KiB、最多 100,000 条记录。重复 JSON 键、未知字段、无效数值或同一运行伪装为多个案例均会拒绝。`load_cases` 与直接调用 `evaluate` 使用同样的校验，不能通过绕开文件加载来放大样本数。

## 指标与默认门槛

- `top1Accuracy` = `correct / (correct + incorrect)`，仅计有具体候选规则且可判定的运行；没有可判定运行时为 `null`。
- `conclusiveCoverage` 保留原口径：可判定运行 / 已审阅运行。它不等于总体覆盖率。
- `samplingFrame` 记录清单是否提供、清单运行总数、已审阅数、未审阅数，以及相对于清单的审阅覆盖率和可判定覆盖率；清单为空时比例为 `null`。
- `top1AccuracyWilson95` 是已观测可判定样本的 Wilson 95% 区间。它不校正样本选择偏差、同业务主体多次运行的相关性或非独立审阅。
- `confidenceCalibration` 保留等宽分箱、Brier 分数和 ECE；不确定标签不参与校准。校准结果也只描述已审阅候选。

`candidateAccuracyAssessment` 默认要求至少 **100 个唯一可判定决策运行**，观测准确率至少 **75%**。少于 100 个为 `insufficient_samples`；样本充足时按未四舍五入的比例输出 `target_met` 或 `target_not_met`。门槛针对观测比例，不要求 Wilson 下界达到 75%。同一运行不能依靠多个归因记录重复计数；不确定标签不计入这 100 个运行。

`--minimum-runs` 和 `--target-accuracy` 只用于诊断阈值敏感性，不改变 `m3Acceptance` 固定展示的 100 例 / 75% 业务要求，也不会将可修改项目标签转换为独立盲审证据。

报告不包含运行 ID、案例 ID、规则 ID、审阅人或业务正文。输入文件仍应按项目权限保存；分享报告时应附抽样范围和统计窗口。

## 仍需取得的 M3 业务验收证据

业务验收应先冻结适用项目、版本、时间窗、抽样规则和归因输出快照，并记录被抽中的全部运行，包含无证据、未审阅和无法判定的案例。不能只挑已标为正确的样本，也不能把 100 条相关运行自动称为独立样本。

随后由独立业务专家按预定标准盲审，保留审阅者独立性、是否见过模型结论或他人标签、分歧裁决及标签变更的证据。对规则级“正确”的定义应事先约定；命中路径候选不直接构成因果证明。至少 100 例独立可判定案例达到 75% Top-1 目标后，仍需审核样本代表性、覆盖率及过程证据。

当前工具不接收可以自行宣称“已独立盲审”的开关，也不提供从现有可修改标签一键生成业务验收结论的模式。真实独立标签与审核流程尚未建立时，M3 仍应记为待验收。
