# DMN 规范实现说明

本项目包含原生 Rust DMN XML 解析、结构校验、FEEL 求值和决策服务执行实现。它支持常见 DMN 决策结构的一个可执行子集，不是对 OMG DMN 1.5 规范及全部 FEEL 语义的完整认证实现。生产使用前应使用本项目的校验和验收用例确认具体模型行为。

## XML 和模型结构

解析器识别 DMN 20230324 命名空间，并兼容 20231201 和 20180521 命名空间。支持的主要定义包含 InputData、Decision、InformationRequirement、KnowledgeRequirement、ItemDefinition、BusinessKnowledgeModel、DecisionService，以及 LiteralExpression、DecisionTable、Context、List、Relation、Conditional、Filter、For、Some、Every、Invocation 和 FunctionDefinition 等表达式结构。

实现会读取决策依赖关系并检查引用、类型引用和结构约束；编译器可生成带项目追溯扩展信息的 DMN XML。对未知表达式或不支持的 XML 结构，不应假设它们会按标准语义执行；某些结构会被保留为 unsupported 并在执行时返回错误。

## 决策表

当前解析并执行以下命中策略：

| Hit policy | 行为要点 |
| --- | --- |
| `UNIQUE` | 最多允许一条规则命中；多条命中是策略违规。 |
| `FIRST` | 使用规则顺序中的第一条匹配规则。 |
| `ANY` | 多条命中时输出必须相同。 |
| `PRIORITY` | 依据输出的 `outputValues` 优先级选择结果。 |
| `RULE ORDER` | 按规则顺序返回匹配结果。 |
| `OUTPUT ORDER` | 按输出值优先级排序匹配结果。 |
| `COLLECT` | 汇总所有命中；聚合类型支持 `SUM`、`MIN`、`MAX`、`COUNT`。 |

规则输入支持 FEEL unary test，包括通配符 `-`、相等测试、比较以及区间等项目实现的测试表达式。多输入列按列匹配。聚合、优先级、输出值列表与结果类型之间存在结构约束，非法组合会在校验或执行时报错。

## FEEL 求值

FEEL 解释器使用 Rust AST 和运行时值实现表达式求值。值类型涵盖 null、boolean、number、string、list、context/map 及日期、时间、日期时间、duration、range 等类型。表达式支持算术、逻辑、比较、变量和路径访问、条件表达式、列表/上下文构造、过滤、迭代和量化等语法；具体支持范围以 `src/feel/parser.rs`、`src/feel/interpreter.rs` 和对应测试为准。

内置函数覆盖字符串、数值、列表、日期时间、类型检查、区间等常用操作。调用、参数类型和日期时区语义可能与其他 DMN 引擎不同；依赖复杂 FEEL 库函数或特定引擎扩展的模型须建立逐例回归。未定义变量、类型不兼容、非有限数值、非法区间和未知函数不会被静默转换成业务默认值。

## 类型和输入

常见 FEEL 类型引用会被解析并在输入、决策输出、上下文条目、函数参数或返回值处校验。ItemDefinition 可声明组件、集合、允许值；运行时支持常见基本类型、复合上下文和列表。命名空间前缀会按实现支持的 FEEL 类型引用规则归一化。不同 XML 命名空间或厂商私有类型名仍需验证。

决策服务输入来自调用 JSON 对象。模型中字段名应与对外契约一致，测试缺失值、null、类型错误、边界值及额外字段行为。不要假定所有 JSON 类型都能无损映射到 FEEL，或将 FEEL 的 null 与缺失输入视为等价。

## 执行和错误

执行器按依赖图计算所需决策并生成结果和可选的结构化执行轨迹。决策表多命中违规、未匹配且无默认值、无效依赖、不可满足类型约束等情况会作为执行错误报告。应用的草稿验证、发布验证和公开服务执行也会使用相应结构与权限门禁。

建议为每个 Decision Service 建立覆盖规则边界、无命中、多命中、命中策略、类型和值域错误及决策依赖顺序的确定性测试用例，并通过版本回归测试保护已有发布行为。

## 验证依据

实现范围可通过以下源码和测试核对：

- `src/dmn/model.rs`：内部模型、支持的命中策略及类型结构。
- `src/dmn/xml.rs`、`src/dmn/serialize.rs`：XML 解析与序列化。
- `src/dmn/runtime.rs`：决策依赖和执行语义。
- `src/feel/parser.rs`、`src/feel/interpreter.rs`：FEEL 语法和求值。
- `src/dmn/tests.rs`、`src/feel/tests.rs`：当前回归覆盖。

测试通过表示仓库夹具覆盖的行为符合实现预期，不代表通过 OMG 一致性套件、其他 DMN 引擎互操作认证或规范全文符合性测试。
