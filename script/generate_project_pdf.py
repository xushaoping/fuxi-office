from pathlib import Path
from xml.sax.saxutils import escape

from reportlab.lib import colors
from reportlab.lib.enums import TA_CENTER, TA_LEFT
from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import ParagraphStyle, getSampleStyleSheet
from reportlab.lib.units import mm
from reportlab.lib.utils import ImageReader
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.platypus import (
    BaseDocTemplate,
    Frame,
    Image,
    KeepTogether,
    PageBreak,
    PageTemplate,
    Paragraph,
    Spacer,
    Table,
    TableStyle,
)


ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "docs" / "project-introduction.pdf"
LOGO = ROOT / "public" / "images" / "fuxi.png"
FONT = "/System/Library/Fonts/STHeiti Medium.ttc"

NAVY = colors.HexColor("#15253D")
SLATE = colors.HexColor("#405A7A")
PURPLE = colors.HexColor("#5B62D6")
LIGHT_PURPLE = colors.HexColor("#EEF0FF")
PALE = colors.HexColor("#F7F9FC")
BORDER = colors.HexColor("#DCE4EF")
TEXT = colors.HexColor("#26364A")
MUTED = colors.HexColor("#64748B")


pdfmetrics.registerFont(TTFont("Fuxi", FONT, subfontIndex=0))
pdfmetrics.registerFontFamily("Fuxi", normal="Fuxi", bold="Fuxi")

styles = getSampleStyleSheet()
styles.add(ParagraphStyle("CoverEyebrow", fontName="Fuxi", fontSize=10, leading=14, textColor=PURPLE, spaceAfter=8, tracking=1.5))
styles.add(ParagraphStyle("CoverTitle", fontName="Fuxi", fontSize=28, leading=35, textColor=NAVY, spaceAfter=8))
styles.add(ParagraphStyle("CoverLead", fontName="Fuxi", fontSize=13, leading=21, textColor=SLATE, spaceAfter=18))
styles.add(ParagraphStyle("CoverMeta", fontName="Fuxi", fontSize=9, leading=13, textColor=MUTED, alignment=TA_LEFT))
styles.add(ParagraphStyle("H1F", fontName="Fuxi", fontSize=19, leading=25, textColor=NAVY, spaceBefore=5, spaceAfter=10, keepWithNext=True))
styles.add(ParagraphStyle("H2F", fontName="Fuxi", fontSize=13, leading=18, textColor=SLATE, spaceBefore=9, spaceAfter=5, keepWithNext=True))
styles.add(ParagraphStyle("BodyF", fontName="Fuxi", fontSize=9.2, leading=15.2, textColor=TEXT, spaceAfter=6))
styles.add(ParagraphStyle("SmallF", fontName="Fuxi", fontSize=8.2, leading=12, textColor=MUTED, spaceAfter=4))
styles.add(ParagraphStyle("CardTitle", fontName="Fuxi", fontSize=10.1, leading=14, textColor=PURPLE, spaceAfter=4))
styles.add(ParagraphStyle("CardBody", fontName="Fuxi", fontSize=8.5, leading=12.5, textColor=TEXT))
styles.add(ParagraphStyle("TableHead", fontName="Fuxi", fontSize=8.7, leading=12, textColor=colors.white))
styles.add(ParagraphStyle("TableCell", fontName="Fuxi", fontSize=8.5, leading=12, textColor=TEXT))
styles.add(ParagraphStyle("CodeF", fontName="Fuxi", fontSize=8.2, leading=12, textColor=TEXT))
styles.add(ParagraphStyle("CenterSmall", fontName="Fuxi", fontSize=8, leading=11, textColor=MUTED, alignment=TA_CENTER))


def p(text, style="BodyF"):
    return Paragraph(text, styles[style])


def plain(text, style="BodyF"):
    return p(escape(text), style)


def bullets(items):
    return [p(f"<font color='{PURPLE}'>•</font> {escape(item)}") for item in items]


def boxed(content, background=PALE, border=BORDER, padding=10, radius=7):
    table = Table([[content]], colWidths=[168 * mm])
    table.setStyle(TableStyle([
        ("BACKGROUND", (0, 0), (-1, -1), background),
        ("BOX", (0, 0), (-1, -1), 0.7, border),
        ("ROUNDEDCORNERS", [radius, radius, radius, radius]),
        ("LEFTPADDING", (0, 0), (-1, -1), padding),
        ("RIGHTPADDING", (0, 0), (-1, -1), padding),
        ("TOPPADDING", (0, 0), (-1, -1), padding),
        ("BOTTOMPADDING", (0, 0), (-1, -1), padding),
    ]))
    return table


def two_column_cards(cards):
    rows = []
    for index in range(0, len(cards), 2):
        row = []
        for card in cards[index:index + 2]:
            title, body = card
            row.append([p(title, "CardTitle"), p(body, "CardBody")])
        if len(row) == 1:
            row.append("")
        rows.append(row)
    table = Table(rows, colWidths=[83 * mm, 83 * mm], hAlign="LEFT")
    table.setStyle(TableStyle([
        ("BACKGROUND", (0, 0), (-1, -1), colors.HexColor("#FBFCFE")),
        ("BOX", (0, 0), (-1, -1), 0.7, BORDER),
        ("INNERGRID", (0, 0), (-1, -1), 0.7, BORDER),
        ("VALIGN", (0, 0), (-1, -1), "TOP"),
        ("LEFTPADDING", (0, 0), (-1, -1), 10),
        ("RIGHTPADDING", (0, 0), (-1, -1), 10),
        ("TOPPADDING", (0, 0), (-1, -1), 9),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 9),
    ]))
    return table


def header_footer(canvas, doc):
    canvas.saveState()
    if doc.page > 1:
        canvas.setStrokeColor(BORDER)
        canvas.setLineWidth(0.5)
        canvas.line(21 * mm, 284 * mm, 189 * mm, 284 * mm)
        canvas.setFont("Fuxi", 7.5)
        canvas.setFillColor(MUTED)
        canvas.drawString(21 * mm, 289 * mm, "伏羲智能决策工作台 · 项目介绍")
        canvas.drawRightString(189 * mm, 12 * mm, f"{doc.page}")
    canvas.restoreState()


def build_story():
    story = []

    story.extend([
        Spacer(1, 20 * mm),
        Image(str(LOGO), width=38 * mm, height=27.6 * mm),
        Spacer(1, 8 * mm),
        p("OPEN SOURCE · AI DECISION AUTOMATION", "CoverEyebrow"),
        p("伏羲智能决策工作台", "CoverTitle"),
        p("让业务规则从自然语言出发，经过结构化建模、确定性校验与人工治理，成为可执行、可解释、可审计的决策服务。", "CoverLead"),
        Spacer(1, 4 * mm),
    ])
    banner = [
        p("核心创新", "SmallF"),
        p("<b>AI 负责理解与建模，规则引擎负责生产决策。</b>", "BodyF"),
        p("把大模型的灵活性与 DMN/FEEL 的确定性组合起来，避免“让模型直接回答业务结果”的不可控风险。", "SmallF"),
    ]
    story.append(boxed(banner, background=colors.HexColor("#263F69"), border=colors.HexColor("#263F69")))
    story.append(Spacer(1, 10 * mm))
    metric_data = [[
        [p("AI → DSL", "CardTitle"), p("自然语言到结构化模型", "CenterSmall")],
        [p("DMN 1.5", "CardTitle"), p("标准化、可移植的决策表达", "CenterSmall")],
        [p("全链路", "CardTitle"), p("版本、验收、运行与反馈闭环", "CenterSmall")],
    ]]
    metric_table = Table(metric_data, colWidths=[55 * mm] * 3)
    metric_table.setStyle(TableStyle([
        ("BACKGROUND", (0, 0), (-1, -1), PALE),
        ("BOX", (0, 0), (-1, -1), 0.7, BORDER),
        ("INNERGRID", (0, 0), (-1, -1), 0.7, BORDER),
        ("VALIGN", (0, 0), (-1, -1), "MIDDLE"),
        ("ALIGN", (0, 0), (-1, -1), "CENTER"),
        ("TOPPADDING", (0, 0), (-1, -1), 9),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 9),
    ]))
    story.extend([metric_table, Spacer(1, 22 * mm), p("项目介绍 · 开源项目 · 2026 年 10 月 17 日", "CoverMeta"), PageBreak()])

    story.extend([
        p("一、项目背景", "H1F"),
        plain("企业的贷款审批、订单优惠、航班延误处置、风控分级等业务，通常依赖制度文件、Excel 规则表和人工经验。传统规则落地存在四类问题："),
        *bullets([
            "业务人员需要同时理解自然语言、技术 DSL、DMN XML 和 FEEL 表达式。",
            "规则从需求到上线跨越建模、开发、测试、审批多个环节，沟通成本高。",
            "规则变更容易引起引用丢失、边界遗漏、版本不可追溯和线上结果难解释。",
            "直接让大模型回答业务结果，存在随机性、幻觉、不可审计和难以回放的问题。",
        ]),
        boxed([p("<b>伏羲的解决思路：</b> 使用 AI Agent 理解和生成结构化决策模型，再由确定性的 DMN/FEEL 引擎进行校验、执行和审计。AI 提升建模效率，规则引擎保证生产可靠性。")], background=LIGHT_PURPLE, border=PURPLE),
        p("二、项目定位与目标", "H1F"),
        plain("伏羲是一个全栈 Rust 实现的企业级 DMN 决策自动化平台。平台将“自然语言需求”转换成“可验证、可版本化、可发布、可观测的决策服务”。"),
        *bullets([
            "让业务用户可以用自然语言描述决策目标和规则。",
            "将需求分析、澄清、方案规划、DSL 生成、DMN 编译和增量修改拆成可控阶段。",
            "以结构化 DSL 作为中间表示，统一连接 Agent、编译器和运行时。",
            "通过多层确定性校验和业务验收，阻止错误模型进入生产。",
            "为发布后的每次决策保留版本、命中规则、执行轨迹和业务反馈。",
            "用业务摘要、澄清问题和可视化模型产物降低非技术用户的建模门槛。",
        ]),
        PageBreak(),
    ])

    story.extend([
        p("三、总体技术架构", "H1F"),
        p("分层架构", "H2F"),
    ])
    layers = [
        ("交互层", "Leptos CSR / WebAssembly 工作台 · 自然语言输入 · 模型与验收可视化"),
        ("服务层", "Axum API · JWT 认证 · 项目级 RBAC · SSE 流式反馈 · 决策服务端点"),
        ("智能层", "阶段化 Agent：需求分析 → 澄清 → 规划确认 → DSL 生成 → 增量修改"),
        ("编译层", "Decision DSL → 结构校验 → DMN 1.5 XML → FEEL 预编译与依赖分析"),
        ("运行层", "确定性 DMN/FEEL 执行 · 规则命中 · 执行轨迹 · 运行分析 · 业务反馈"),
        ("数据层", "PostgreSQL：项目、版本、制品、测试、审计、知识源与运行记录"),
    ]
    layer_data = [[p(name, "CardTitle"), p(body, "TableCell")] for name, body in layers]
    layer_table = Table(layer_data, colWidths=[27 * mm, 141 * mm])
    layer_table.setStyle(TableStyle([
        ("BACKGROUND", (0, 0), (0, -1), LIGHT_PURPLE),
        ("BACKGROUND", (1, 0), (1, -1), PALE),
        ("BOX", (0, 0), (-1, -1), 0.7, BORDER),
        ("INNERGRID", (0, 0), (-1, -1), 0.7, BORDER),
        ("VALIGN", (0, 0), (-1, -1), "MIDDLE"),
        ("LEFTPADDING", (0, 0), (-1, -1), 9),
        ("RIGHTPADDING", (0, 0), (-1, -1), 9),
        ("TOPPADDING", (0, 0), (-1, -1), 7),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 7),
    ]))
    story.extend([layer_table, Spacer(1, 5 * mm), p("关键数据流", "H2F"), boxed([plain("用户 / 业务系统 → 工作台 → 阶段化 Agent → Decision DSL → DMN 1.5 XML → FEEL 运行时 → 结果、轨迹与反馈", "BodyF")]), p("技术栈", "H2F")])
    stack = [
        [p("领域", "TableHead"), p("实现", "TableHead")],
        [plain("后端", "TableCell"), plain("Rust 2024、Axum 0.8、Tokio、SQLx、PostgreSQL", "TableCell")],
        [plain("前端", "TableCell"), plain("Leptos 0.8 CSR、WebAssembly、leptos_router", "TableCell")],
        [plain("智能体", "TableCell"), plain("OpenAI Chat Completions 兼容接口、结构化工具调用、SSE", "TableCell")],
        [plain("决策引擎", "TableCell"), plain("DMN 1.5、FEEL 词法分析、递归下降解析、解释执行", "TableCell")],
        [plain("工程治理", "TableCell"), plain("同源图片验证码、JWT、Argon2、项目级 RBAC、版本审查、审计和 Docker 部署", "TableCell")],
    ]
    stack_table = Table(stack, colWidths=[27 * mm, 141 * mm])
    stack_table.setStyle(TableStyle([
        ("BACKGROUND", (0, 0), (-1, 0), SLATE),
        ("BACKGROUND", (0, 1), (-1, -1), colors.white),
        ("BOX", (0, 0), (-1, -1), 0.7, BORDER),
        ("INNERGRID", (0, 0), (-1, -1), 0.7, BORDER),
        ("VALIGN", (0, 0), (-1, -1), "MIDDLE"),
        ("LEFTPADDING", (0, 0), (-1, -1), 8),
        ("RIGHTPADDING", (0, 0), (-1, -1), 8),
        ("TOPPADDING", (0, 0), (-1, -1), 6),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 6),
    ]))
    story.extend([stack_table, PageBreak()])

    story.extend([p("四、核心功能与应用场景", "H1F")])
    feature_cards = [
        ("业务友好的需求分析", "用业务摘要、关键规则、输入输出、风险和澄清问题呈现分析结果，突出需要确认的重点。"),
        ("阶段化智能建模", "需求与方案、决策建模分为两个工作区 Tab，依次完成需求分析、澄清、规划、DSL 生成和 DMN 编译。"),
        ("增量修改", "以原始需求和当前 DSL 为受控上下文，生成新的 DSL 并编译 DMN，保护未变更元素并检查影响范围。"),
        ("企业知识增强", "绑定术语、字段目录、制度政策和历史模型，记录引用元数据并固化版本知识快照。"),
        ("模型产物可视化", "提供 YAML 结构树、搜索、折叠和预览，并对 YAML、DSL 与 DMN/XML 源码进行语法高亮。"),
        ("DMN/FEEL 引擎", "支持决策表、字面量表达式、上下文、关系、条件、过滤器、迭代器、函数调用和服务。"),
        ("安全与治理", "登录使用服务端生成的同源图片验证码，并提供角色权限、版本差异、风险识别、回滚草稿和审计日志。"),
        ("验收、发布与分析", "覆盖边界、缺失、类型错误、冲突和无命中案例，完成业务批准后发布不可变版本。"),
    ]
    story.extend([two_column_cards(feature_cards), Spacer(1, 6 * mm), p("典型应用场景", "H2F")])
    scene_cards = [
        ("零售贷款审批", "输入年龄、收入、信用分等信息，计算债务收入比并输出 Approve、Reject 或 ManualReview；适合展示规则解释、边界测试与版本回溯。"),
        ("订单折扣计算", "根据客户等级、订单金额、商品类别、促销状态和库存情况，计算折扣比例、优惠金额或人工审核结果。"),
        ("航班延误处置", "根据延误时长、天气、航线、旅客类型和机场状态，生成改签、退款、住宿、餐食或人工升级方案。"),
        ("风控与合规", "结合外部风险评分、客户标签、制度政策和历史模型，执行风险分级、准入判断与人工升级。"),
    ]
    story.extend([two_column_cards(scene_cards), PageBreak()])

    story.extend([
        p("五、项目创新点", "H1F"),
        boxed([p("<b>为什么不是普通的 LLM + 业务系统？</b>"), plain("伏羲把“生成”和“执行”彻底解耦。模型帮助人快速把制度和经验整理成候选规则，但任何生产结果都必须经过 schema、类型、依赖、编译、验收和发布门禁，再由确定性引擎执行。")], background=colors.HexColor("#263F69"), border=colors.HexColor("#263F69")),
        Spacer(1, 4 * mm),
        two_column_cards([
            ("01 · 生成与执行解耦", "Agent 只生成结构化候选模型；DMN/FEEL 引擎决定最终结果，兼顾效率、稳定性与可回放。"),
            ("02 · DSL 可审计中间层", "用比 XML 更适合生成与修改的 DSL 连接 Agent、编译器和运行时，支持规范化、校验与哈希。"),
            ("03 · 阶段化人在回路", "分析、澄清、规划、生成和验收各有状态与检查点，在关键节点保留业务确认。"),
            ("04 · 增量修改安全护栏", "局部调整只影响授权元素；删除、重命名、命名空间或依赖变化会被显式识别。"),
            ("05 · 业务验收闭环", "自动产生边界和异常案例，由引擎执行、业务批准后才能发布；线上反馈反哺规则优化。"),
            ("06 · 全链路可追溯", "从 Agent 输入、DSL、DMN 制品、测试、审批到运行轨迹和业务反馈，形成审计证据链。"),
        ]),
        Spacer(1, 5 * mm),
        plain("这套架构的关键价值在于：把 LLM 的不确定性限制在“候选生成”边界内，把生产系统最需要的可重复、可验证、可回滚和可解释交给确定性工程。"),
        PageBreak(),
    ])

    story.extend([p("六、项目演示", "H1F"), p("在线地址", "H2F"), p('<link href="https://fuxioffice.cn/" color="#4D56C7"><b>https://fuxioffice.cn/</b></link>'), p("演示账号", "H2F")])
    account_data = [
        [p("角色", "TableHead"), p("用户名", "TableHead"), p("密码", "TableHead")],
        [plain("管理员", "TableCell"), plain("admin", "TableCell"), plain("admin1234", "TableCell")],
        [plain("普通用户", "TableCell"), plain("fuxi", "TableCell"), plain("admin1234", "TableCell")],
    ]
    account_table = Table(account_data, colWidths=[56 * mm, 56 * mm, 56 * mm])
    account_table.setStyle(TableStyle([
        ("BACKGROUND", (0, 0), (-1, 0), SLATE),
        ("BACKGROUND", (0, 1), (-1, -1), PALE),
        ("BOX", (0, 0), (-1, -1), 0.7, BORDER),
        ("INNERGRID", (0, 0), (-1, -1), 0.7, BORDER),
        ("LEFTPADDING", (0, 0), (-1, -1), 9),
        ("RIGHTPADDING", (0, 0), (-1, -1), 9),
        ("TOPPADDING", (0, 0), (-1, -1), 7),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 7),
    ]))
    story.extend([account_table, Spacer(1, 3 * mm), p("以上为公开演示账号；正式部署时请立即修改初始密码，并为不同操作者分配独立账号。", "SmallF"), p("推荐演示路径", "H2F")])
    steps = [
        ("1", "进入工作台", "打开线上地址并登录，进入项目空间；优先使用管理员账号观察完整菜单与治理能力。"),
        ("2", "需求与方案", "输入贷款审批需求，观察 Agent 完成需求分析、澄清、方案规划；分析完成后页面自动定位到待确认问题。"),
        ("3", "决策建模", "确认方案后切换到决策建模，生成 DSL 并编译为标准 DMN 1.5 XML；查看 YAML 结构和源码高亮。"),
        ("4", "验收与发布", "执行自动生成的边界/异常案例，查看命中规则与结果；业务批准后发布版本。"),
        ("5", "运行与追溯", "输入测试 JSON 调用已发布决策，查看输出、执行轨迹、耗时、版本和业务反馈。"),
        ("6", "局部变更", "在当前 DSL 和原始需求上下文中提出“将风险阈值从 0.55 调整为 0.50”，重新生成 DSL/DMN 后对比变更审查和回滚草稿。"),
    ]
    step_cards = []
    for number, title, body in steps:
        step_cards.append((f"{number} · {title}", body))
    story.extend([two_column_cards(step_cards), Spacer(1, 5 * mm), p("推荐演示输入", "H2F")])
    prompt = "创建一个名为 RetailLoanApproval 的零售贷款审批模型。输入 ApplicantAge、AnnualIncome、CreditScore、MonthlyDebtPayment、RequestedAmount、EmploymentMonths 和 ExistingCustomer。计算 DebtToIncomeRatio；CreditScore 小于 600 或 DebtToIncomeRatio 大于 0.55 为 High，小于 720 或 EmploymentMonths 小于 12 为 Medium，其他为 Low。最终输出 Approve、Reject 或 ManualReview，并保留所有决策依赖。"
    story.append(boxed([plain(prompt, "SmallF")], background=PALE, border=BORDER))
    story.extend([Spacer(1, 4 * mm), p("推荐测试输入", "H2F")])
    story.append(boxed([p('{"ApplicantAge": 32, "AnnualIncome": 120000, "CreditScore": 760, "MonthlyDebtPayment": 1800, "RequestedAmount": 200000, "EmploymentMonths": 48, "ExistingCustomer": true}', "CodeF")], background=PALE, border=BORDER))
    story.append(PageBreak())

    story.extend([
        p("七、开源项目价值", "H1F"),
        *bullets([
            "对业务人员：用业务语言建模，减少学习 DMN、XML 和 FEEL 的成本。",
            "对研发团队：结构化 DSL、编译器和测试门禁降低规则代码维护成本。",
            "对管理与合规：版本、审批、审计、回滚和执行轨迹形成治理闭环。",
            "对平台建设：Rust 单体二进制易部署，领域模块清晰，便于后续拆分服务。",
            "对 AI 应用研究：提供“LLM 生成 + 确定性运行时 + 人在回路”的可落地样例。",
        ]),
        p("八、代码与文档索引", "H1F"),
        *bullets([
            "Agent 与工作流：src/nl_dsl/agent.rs、src/workflow.rs、src/workflow_api.rs。",
            "DSL 与 DMN：src/nl_dsl/dsl.rs、src/nl_dsl/dmn.rs、src/dmn/compile.rs。",
            "FEEL 与运行时：src/feel/、src/dmn/runtime.rs。",
            "轨迹与分析：src/execution_trace.rs、src/decision_analytics.rs。",
            "版本与权限：src/version_review.rs、src/version_verification.rs、src/project_access.rs。",
        ]),
        boxed([p("<b>结语：</b> 伏羲不是让 AI 替代规则系统，而是让 AI 成为规则系统的自然语言建模入口。通过“Agent 理解、DSL 表达、编译器校验、确定性执行、人工治理、运行反馈”的组合，企业可以把分散在文档和经验中的决策知识，逐步沉淀为可执行、可解释、可审计的数字化资产。")], background=LIGHT_PURPLE, border=PURPLE),
        Spacer(1, 15 * mm),
        p("线上演示：<link href=\"https://fuxioffice.cn/\" color=\"#4D56C7\">https://fuxioffice.cn/</link> · 文档版本 2026-10-17", "CenterSmall"),
    ])
    return story


def main():
    doc = BaseDocTemplate(
        str(OUTPUT),
        pagesize=A4,
        leftMargin=21 * mm,
        rightMargin=21 * mm,
        topMargin=20 * mm,
        bottomMargin=18 * mm,
        title="伏羲智能决策工作台 - 项目介绍",
        author="伏羲智能决策工作台",
    )
    frame = Frame(doc.leftMargin, doc.bottomMargin, doc.width, doc.height, id="normal")
    doc.addPageTemplates([PageTemplate(id="all", frames=[frame], onPage=header_footer)])
    doc.build(build_story())
    print(f"wrote {OUTPUT}")


if __name__ == "__main__":
    main()
