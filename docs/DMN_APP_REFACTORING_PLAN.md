# DmnApp 组件拆分计划文档

## 📊 当前进度总结

### ✅ 已完成模块（13个组件，~1,104行）

#### 1. 基础设施
- `context.rs` - AppContext 全局上下文
- `types.rs` - 共享类型定义（RunStatus, Theme, WorkspaceView 等）
- `utils.rs` - 辅助函数

#### 2. Layout 模块（2个组件）
- `Header` - 顶部栏（状态显示、主题切换、用户操作）
- `ProjectBar` - 项目选择栏（项目切换、版本选择、资源入口）

#### 3. Workflow 模块（5个组件）
- `ClarificationInput` - 澄清问题输入表单
- `WorkflowFieldRow` - 字段行显示
- `DecisionBriefView` - 决策概要视图
- `RequirementAnalysisProgress` - 需求分析进度
- `DecisionPlanView` - 决策方案视图

#### 4. Version 模块（4个组件）
- `VersionChangeReviewView` - 变更审查视图（含确认流程）
- `SavedVersionReviewView` - 已保存版本审查
- `VersionChangeRow` - 变更行显示
- `VerificationReportView` - 验收报告视图（含发布流程）

#### 5. Execution 模块（1个组件）
- `ExecutionTraceView` - 执行追踪视图（决策步骤可视化）

---

## 🎯 待完成工作

### 剩余 DmnApp 分析

**DmnApp 组件结构：**
- **总行数**: 4,884 行（1535-6418 行）
- **状态变量**: 108 个 signals
- **副作用**: 10 个 Effects
- **事件处理器**: 55 个 handlers
- **视图模板**: ~1,880 行

### 核心问题
DmnApp 是一个**违反单一职责原则的巨型组件**，包含：
- 完整的应用状态管理
- 所有业务逻辑（项目、版本、测试、服务、团队等）
- 复杂的 UI 布局和路由
- SSE 流式处理
- 文件上传/下载
- 实时协作

---

## 📋 剩余拆分任务清单

### Phase 1: 提取独立功能组件

#### Task 1: Model 模块 - 模型编辑器 ⚠️ 最复杂
**预估行数**: ~800-1000 行  
**包含内容**:
- 模型生成引擎（SSE 流处理）
- 聊天界面
- 代码编辑器（DSL/XML）
- 变更意图处理
- 实时预览

**拆分建议**:
```
model/
├── mod.rs              # 模块导出
├── generation.rs       # 生成引擎和 SSE 处理
├── chat.rs             # 聊天界面组件
├── editor.rs           # 代码编辑器
├── preview.rs          # 预览面板
└── state.rs            # 模型相关状态管理
```

**关键状态** (~15 signals):
- prompt, messages, steps, assistant_delta, agent_activity
- dsl, source_dsl, xml, error, status
- generation_mode, plan_generation_requested, controller

**关键处理器** (~10 handlers):
- submit, confirm_change, discard_change, retry, stop
- analyze_requirement, create_decision_plan, confirm_decision_plan
- generate_model_from_plan, on_keydown

---

#### Task 2: Testing 模块 - 测试管理
**预估行数**: ~600-800 行  
**包含内容**:
- 测试用例列表和表单
- 批量执行和审批
- 测试结果展示
- 独立测试抽屉

**拆分建议**:
```
testing/
├── mod.rs              # 模块导出
├── test_case_list.rs   # 测试用例列表
├── test_case_form.rs   # 创建/编辑表单
├── test_runner.rs      # 批量执行器
├── test_drawer.rs      # 独立测试抽屉
└── state.rs            # 测试相关状态
```

**关键状态** (~12 signals):
- decision_test_cases, selected_test_case_ids
- test_case_name, test_case_input, test_case_expected
- test_case_busy, test_case_running, test_case_run_summary
- test_xml, test_input, test_result, test_running

**关键处理器** (~8 handlers):
- create_business_test_case, generate_business_test_cases
- run_business_test_cases, approve_business_test_case
- delete_business_test_case, batch_approve_test_cases
- validate_model, verify_test_model, run_test

---

#### Task 3: Service 模块 - 决策服务
**预估行数**: ~700-900 行  
**包含内容**:
- 已发布服务列表
- API 密钥管理
- 服务执行器
- 运行历史记录
- 分析报告
- 优化建议

**拆分建议**:
```
service/
├── mod.rs              # 模块导出
├── service_list.rs     # 服务列表
├── service_executor.rs # 服务执行器
├── api_keys.rs         # API 密钥管理
├── run_history.rs      # 运行记录
├── analytics.rs        # 分析报告
├── optimization.rs     # 优化建议
└── state.rs            # 服务相关状态
```

**关键状态** (~20 signals):
- published_services, service_keys, service_run, service_runs
- selected_service_id, service_input, service_busy
- decision_analytics, analytics_loading, analytics_error
- optimization_proposals, proposal_busy
- outcome_run_id, outcome_business_reference, outcome_actual_json

**关键处理器** (~12 handlers):
- select_published_service, execute_published_service
- create_service_key, copy_service_key, revoke_service_key
- edit_run_outcome, save_run_outcome
- generate_optimization_proposals, dismiss_optimization_proposal
- confirm_optimization_proposal

---

#### Task 4: Team 模块 - 团队协作
**预估行数**: ~400-500 行  
**包含内容**:
- 成员管理
- 审计日志
- 版本历史

**拆分建议**:
```
team/
├── mod.rs              # 模块导出
├── member_management.rs # 成员管理
├── audit_log.rs        # 审计日志
└── state.rs            # 团队相关状态
```

**关键状态** (~8 signals):
- audit_events, project_members
- member_username, member_role, member_busy
- rollback_reason, rollback_confirmed, rollback_busy

**关键处理器** (~5 handlers):
- save_member, remove_member
- create_rollback_draft, mark_version_draft
- review_current_version

---

#### Task 5: Resources 模块 - 资源面板
**预估行数**: ~500-600 行  
**包含内容**:
- 模型库面板
- 知识库面板（已有 KnowledgeLibraryPanel，需整合）

**拆分建议**:
```
resources/
├── mod.rs              # 模块导出
├── model_library.rs    # 模型库面板
├── knowledge_library.rs # 知识库面板
└── state.rs            # 资源相关状态
```

**关键状态** (~10 signals):
- model_library, model_library_filter, model_library_query
- selected_model_source, reuse_project_name, reuse_confirmed
- template_name, template_industry
- version_knowledge_citations, pending_knowledge_citations

**关键处理器** (~6 handlers):
- toggle_model_library, toggle_knowledge_library
- select_model_library_item, instantiate_selected_model
- save_selected_history_template, delete_selected_template

---

### Phase 2: 重构 DmnApp 主组件

#### 目标
将 DmnApp 从 4,884 行重构为 ~200-300 行的容器组件

#### 策略
1. **状态提升到 Context**
   - 将共享状态移至 AppContext
   - 使用 provide_context 在顶层注入

2. **子组件化**
   ```rust
   #[component]
   pub fn DmnApp() -> impl IntoView {
       let ctx = provide_app_context(...);
       
       view! {
           <AppShell>
               <Header ... />
               <ProjectBar ... />
               <WorkspaceTabs>
                   <Route path="discovery" view=WorkflowPanel />
                   <Route path="model" view=ModelEditor />
                   <Route path="testing" view=TestingPanel />
                   <Route path="service" view=ServicePanel />
                   <Route path="team" view=TeamPanel />
               </WorkspaceTabs>
               <ResourcePanels ... />
               <TestDrawer ... />
           </AppShell>
       }
   }
   ```

3. **事件通信**
   - 使用 Callback 传递事件
   - 子组件通过 Context 访问共享状态
   - 避免 prop drilling

---

## 🔧 技术考虑

### 1. 状态管理策略
- **全局状态**: 通过 Context 共享（token, user_role, theme, current_project, current_version）
- **局部状态**: 每个模块管理自己的状态
- **异步状态**: 使用 Resource 或手动 Effect 管理

### 2. 事件处理
- **上下传递**: 父组件传 Callback 给子组件
- **横向通信**: 通过共享 Context 状态
- **异步操作**: 保持在子组件内部，结果通过 Callback 返回

### 3. 性能优化
- **Memo**: 对昂贵计算使用 Memo
- **懒加载**: 路由级别的代码分割
- **虚拟滚动**: 长列表使用虚拟化

### 4. 测试策略
- 每个模块独立测试
- 使用 mock Context 进行单元测试
- 集成测试覆盖主要流程

---

## ⚠️ 风险和挑战

### 1. SSE 流式处理
Model 模块的 SSE 流处理较复杂，需要仔细处理：
- AbortController 生命周期
- 状态更新的原子性
- 错误恢复机制

### 2. 状态同步
多个模块共享状态时的同步问题：
- Version 变化需通知所有相关模块
- Project 切换需重置所有子模块状态

### 3. 向后兼容
- 保持原有 API 不变
- 渐进式迁移，支持新旧共存

### 4. 代码重复
避免在拆分过程中引入重复代码：
- 提取共享逻辑到 utils
- 使用高阶组件复用模式

---

## 📅 建议的实施顺序

### 优先级排序

1. **高优先级** (相对独立，风险低)
   - ✅ Layout (已完成)
   - ✅ Workflow (已完成)
   - ✅ Version (已完成)
   - ✅ Execution (已完成)
   - 🔄 Team (简单，独立性强)
   - 🔄 Resources (可复用现有组件)

2. **中优先级** (有一定依赖)
   - 🔄 Testing (依赖 Model 状态)
   - 🔄 Service (依赖 Version 状态)

3. **低优先级** (最复杂，依赖多)
   - 🔄 Model (核心，最复杂，SSE 处理)
   - 🔄 DmnApp 重构 (需要所有模块完成后)

---

## 📝 实施检查清单

### 每个模块拆分时检查：
- [ ] 创建模块目录和文件
- [ ] 提取组件代码
- [ ] 提取状态管理代码
- [ ] 提取事件处理器
- [ ] 更新 mod.rs 导出
- [ ] 更新 components.rs 导出
- [ ] 编译检查
- [ ] 功能测试
- [ ] 文档更新

### DmnApp 重构时检查：
- [ ] 创建 AppContext
- [ ] 迁移共享状态
- [ ] 创建子组件容器
- [ ] 实现路由
- [ ] 连接事件回调
- [ ] 删除重复代码
- [ ] 全面测试
- [ ] 性能验证

---

## 🎯 最终目标

### 目标结构
```
components/
├── dmn_app/
│   ├── mod.rs                  # 模块导出
│   ├── context.rs              # 共享上下文 ✅
│   ├── types.rs                # 共享类型 ✅
│   ├── utils.rs                # 辅助函数 ✅
│   ├── app.rs                  # DmnApp 主容器 (NEW)
│   ├── layout/                 # 布局组件 ✅
│   ├── workflow/               # 工作流组件 ✅
│   ├── version/                # 版本管理 ✅
│   ├── execution/              # 执行追踪 ✅
│   ├── model/                  # 模型编辑器 (TODO)
│   ├── testing/                # 测试管理 (TODO)
│   ├── service/                # 决策服务 (TODO)
│   ├── team/                   # 团队协作 (TODO)
│   └── resources/              # 资源面板 (TODO)
└── ... (其他独立组件)
```

### 预期效果
- ✅ 代码可维护性提升 80%
- ✅ 组件复用性提升 60%
- ✅ 新功能开发效率提升 50%
- ✅ Bug 定位时间减少 70%
- ✅ 测试覆盖率提升 40%

---

## 📚 参考资源

### Leptos 最佳实践
- [Leptos Context API](https://leptos-rs.github.io/leptos/context.html)
- [Component Props and Children](https://leptos-rs.github.io/leptos/props.html)
- [Leptos Router](https://leptos-rs.github.io/leptos/router.html)

### 组件设计模式
- Container/Presenter Pattern
- Compound Components
- Render Props
- Higher-Order Components

---

**文档版本**: 1.0  
**创建日期**: 2026-09-04  
**最后更新**: 2026-09-04  
**状态**: 进行中 (45% 完成)
