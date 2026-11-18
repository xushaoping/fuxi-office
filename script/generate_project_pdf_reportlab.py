#!/usr/bin/env python3
"""
使用 reportlab 生成伏羲智能决策工作台产品介绍 PDF
"""

from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import getSampleStyleSheet, ParagraphStyle
from reportlab.lib.units import cm
from reportlab.lib.enums import TA_LEFT, TA_CENTER, TA_JUSTIFY
from reportlab.platypus import SimpleDocTemplate, Paragraph, Spacer, PageBreak, Table, TableStyle
from reportlab.lib import colors
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
import sys
from pathlib import Path

def create_pdf(output_file):
    """创建 PDF 文档"""

    # 创建 PDF 文档
    doc = SimpleDocTemplate(
        str(output_file),
        pagesize=A4,
        rightMargin=2*cm,
        leftMargin=2*cm,
        topMargin=2*cm,
        bottomMargin=2*cm
    )

    # 获取样式
    styles = getSampleStyleSheet()

    # 自定义样式
    title_style = ParagraphStyle(
        'CustomTitle',
        parent=styles['Heading1'],
        fontSize=28,
        textColor=colors.HexColor('#0066cc'),
        spaceAfter=30,
        alignment=TA_CENTER,
        leading=36
    )

    heading1_style = ParagraphStyle(
        'CustomHeading1',
        parent=styles['Heading1'],
        fontSize=18,
        textColor=colors.HexColor('#0066cc'),
        spaceAfter=12,
        spaceBefore=20,
        leading=24
    )

    heading2_style = ParagraphStyle(
        'CustomHeading2',
        parent=styles['Heading2'],
        fontSize=14,
        textColor=colors.HexColor('#0088cc'),
        spaceAfter=10,
        spaceBefore=15,
        leading=18
    )

    body_style = ParagraphStyle(
        'CustomBody',
        parent=styles['BodyText'],
        fontSize=10,
        leading=15,
        alignment=TA_JUSTIFY,
        spaceAfter=8
    )

    bullet_style = ParagraphStyle(
        'CustomBullet',
        parent=styles['BodyText'],
        fontSize=10,
        leading=15,
        leftIndent=20,
        spaceAfter=6
    )

    # 构建文档内容
    story = []

    # 封面
    story.append(Spacer(1, 3*cm))
    story.append(Paragraph("伏羲智能决策工作台", title_style))
    story.append(Spacer(1, 0.5*cm))
    story.append(Paragraph("OPEN SOURCE · AI DECISION AUTOMATION",
                          ParagraphStyle('subtitle', parent=body_style, alignment=TA_CENTER, fontSize=12)))
    story.append(Spacer(1, 1*cm))
    story.append(Paragraph("让业务规则从自然语言出发，经过多智能体协作、结构化建模、<br/>确定性校验与人工治理，成为可执行、可解释、可审计的决策服务",
                          ParagraphStyle('subtitle2', parent=body_style, alignment=TA_CENTER, fontSize=11)))
    story.append(Spacer(1, 2*cm))
    story.append(Paragraph("<b>版本 v0.2.0</b> · 2026-11-18",
                          ParagraphStyle('version', parent=body_style, alignment=TA_CENTER, textColor=colors.grey)))
    story.append(PageBreak())

    # 核心创新
    story.append(Paragraph("核心创新", heading1_style))
    innovations = [
        ("多智能体协作", "Analyst、Architect、Engineer、Tester 并行工作，心跳监控与失联恢复"),
        ("RAG 语义检索", "向量检索 + 关键词检索 + RRF 融合，自动注入相关上下文"),
        ("L3 闭环自动化", "从 L2（单点辅助）升级到 L3（半自动闭环）：漂移检测 → 优化建议 → Shadow 评估"),
        ("DMN 1.5 标准", "标准化、可移植的决策表达，确定性执行引擎保证生产可靠性"),
        ("全链路治理", "版本、验收、运行、反馈、解释、归因完整闭环"),
    ]
    for title, desc in innovations:
        story.append(Paragraph(f"<b>{title}</b>: {desc}", bullet_style))
    story.append(Spacer(1, 0.5*cm))

    # 一、项目背景
    story.append(Paragraph("一、项目背景", heading1_style))
    story.append(Paragraph("企业的贷款审批、订单优惠、航班延误处置、风控分级等业务，通常依赖制度文件、Excel 规则表和人工经验。传统规则落地存在四类问题：", body_style))

    problems = [
        "<b>认知门槛高</b>：业务人员需要同时理解自然语言、技术 DSL、DMN XML 和 FEEL 表达式",
        "<b>沟通成本高</b>：规则从需求到上线跨越建模、开发、测试、审批多个环节",
        "<b>维护风险大</b>：变更容易引起引用丢失、边界遗漏、版本不可追溯和线上结果难解释",
        "<b>LLM 直接决策不可控</b>：直接让大模型回答业务结果，存在随机性、幻觉、不可审计和难以回放的问题",
    ]
    for problem in problems:
        story.append(Paragraph(f"• {problem}", bullet_style))

    story.append(Spacer(1, 0.3*cm))
    story.append(Paragraph("<b>伏羲的解决思路</b>：使用多智能体协作理解和生成结构化决策模型，再由确定性的 DMN/FEEL 引擎进行校验、执行和审计。AI 提升建模效率，规则引擎保证生产可靠性。", body_style))
    story.append(Spacer(1, 0.5*cm))

    # 二、项目定位与目标
    story.append(Paragraph("二、项目定位与目标（v0.2.0 升级）", heading1_style))
    story.append(Paragraph('伏羲是一个全栈 Rust 实现的企业级 DMN 决策自动化平台，<b>智能化成熟度已达 L3（半自动闭环）</b>。平台将"自然语言需求"转换成"可验证、可版本化、可发布、可观测的决策服务"。', body_style))

    story.append(Paragraph("核心能力", heading2_style))
    capabilities = [
        "<b>多智能体协作编排</b>：Analyst、Architect、Engineer、Tester 等多角色并行工作，支持心跳监控、失联恢复、取消传播",
        "<b>RAG 知识增强</b>：知识分块（约 400 token）、向量索引、混合检索（关键词+向量+RRF），自动注入相关上下文",
        "<b>阶段化人在回路</b>：需求分析 → 澄清确认 → 方案规划 → 模型生成，每步可控可验证",
        "<b>结构化中间层</b>：以 DSL 作为中间表示，统一连接 Agent、编译器和运行时",
        "<b>多层确定性校验</b>：DSL → DMN 编译 → FEEL 语法 → 依赖完整性 → 契约测试 → 历史回归",
        "<b>完整运营闭环</b>：决策解释、偏差归因、漂移检测、优化建议、Shadow 评估、效果追踪",
        "<b>全链路可追溯</b>：从 Agent 输入、DSL、DMN 制品、测试、审批到运行轨迹和业务反馈",
    ]
    for cap in capabilities:
        story.append(Paragraph(f"• {cap}", bullet_style))
    story.append(PageBreak())

    # 三、总体技术架构
    story.append(Paragraph("三、总体技术架构（v0.2.0 升级）", heading1_style))

    story.append(Paragraph("分层架构", heading2_style))
    arch_layers = [
        ("<b>交互层</b>", "Leptos CSR / WebAssembly 工作台 · 自然语言输入 · 运营 Copilot"),
        ("<b>服务层</b>", "Axum API · JWT 认证 · 项目级 RBAC · SSE 流式反馈"),
        ("<b>智能层</b>", "多智能体协作编排 · RAG 语义检索 · 阶段化工作流 · Tester Agent"),
        ("<b>编译层</b>", "Decision DSL → 结构校验 → DMN 1.5 XML → FEEL 预编译"),
        ("<b>运行层</b>", "确定性 DMN/FEEL 执行 · 决策解释 · 偏差归因 · 漂移检测 · 优化闭环"),
        ("<b>数据层</b>", "PostgreSQL 14+ / pgvector · 按月分区 · 日粒度物化视图 · 三角色权限隔离"),
    ]
    for layer, desc in arch_layers:
        story.append(Paragraph(f"{layer}: {desc}", bullet_style))

    story.append(Spacer(1, 0.5*cm))
    story.append(Paragraph("技术栈", heading2_style))

    # 技术栈表格
    tech_data = [
        ['领域', '实现'],
        ['后端', 'Rust 1.90 · Axum 0.8 · Tokio · SQLx · PostgreSQL 14+'],
        ['前端', 'Leptos 0.8 CSR · WebAssembly · leptos_router'],
        ['智能体', 'OpenAI/Anthropic 兼容 · 多智能体编排 · RAG 检索'],
        ['决策引擎', 'DMN 1.5 · FEEL 1.3 · 递归下降解析 · 解释执行'],
        ['工程治理', 'hCaptcha · JWT · Argon2 · RBAC · 提示词版本管理'],
    ]

    tech_table = Table(tech_data, colWidths=[3.5*cm, 12*cm])
    tech_table.setStyle(TableStyle([
        ('BACKGROUND', (0, 0), (-1, 0), colors.HexColor('#0066cc')),
        ('TEXTCOLOR', (0, 0), (-1, 0), colors.whitesmoke),
        ('ALIGN', (0, 0), (-1, -1), 'LEFT'),
        ('FONTNAME', (0, 0), (-1, 0), 'Helvetica-Bold'),
        ('FONTSIZE', (0, 0), (-1, 0), 10),
        ('BOTTOMPADDING', (0, 0), (-1, 0), 12),
        ('BACKGROUND', (0, 1), (-1, -1), colors.white),
        ('GRID', (0, 0), (-1, -1), 1, colors.grey),
        ('FONTSIZE', (0, 1), (-1, -1), 9),
        ('ROWBACKGROUNDS', (0, 1), (-1, -1), [colors.white, colors.HexColor('#f9f9f9')]),
    ]))
    story.append(tech_table)
    story.append(PageBreak())

    # 四、v0.2.0 核心创新点
    story.append(Paragraph("四、v0.2.0 核心创新点", heading1_style))

    innovations_v2 = [
        ("01 · 多智能体协作编排（L2 → L3 升级）",
         "不再是单一 Agent 线性流水线，而是多角色并行协作，支持心跳监控、失联恢复、取消传播。"),
        ("02 · RAG 语义检索（知识增强升级）",
         "从全文注入升级到分块 + 向量索引 + 混合检索（关键词 + 向量 + RRF），显著提升检索准确率。"),
        ("03 · 决策解释生成（可解释性）",
         "按需生成自然语言解释，支持版本绑定、盲审标注、可读性评估，提升业务信任度。"),
        ("04 · 偏差归因闭环（反馈闭环）",
         "批量回传业务结果 → 智能归因候选 → 人工复核 → 归因评测，形成完整反馈闭环。"),
        ("05 · 漂移检测与事件驱动（自动化运维）",
         "每日自动扫描失败率、偏差率、输入分布、规则命中率，事件驱动触发优化提案。"),
        ("06 · 优化建议真实化（闭环完整化）",
         "从占位实现升级到：生成建议 → 历史样本模拟 → Shadow 资格评估 → 自动创建草稿 → 效果追踪。"),
        ("07 · 运营 Copilot（自然语言运维）",
         "自然语言查询项目指标，意图识别与澄清，降低运维门槛。"),
        ("08 · 提示词版本管理（资产化）",
         "提示词版本控制、Golden 评测、激活回滚、审计历史，提升提示词质量与稳定性。"),
        ("09 · 数据存储优化（性能与治理）",
         "按月分区存储、日粒度物化视图、三角色权限隔离，提升查询性能与数据安全。"),
        ("10 · 完整评测体系（质量保障）",
         "RAG 检索、Agent 编排、偏差归因、漂移检测、决策解释、Tester 提案，全部提供独立评测工具与 Wilson 95% 置信区间。"),
    ]

    for title, desc in innovations_v2:
        story.append(Paragraph(f"<b>{title}</b>", heading2_style))
        story.append(Paragraph(desc, body_style))
        story.append(Spacer(1, 0.2*cm))

    story.append(PageBreak())

    # 五、项目演示
    story.append(Paragraph("五、项目演示", heading1_style))

    story.append(Paragraph("在线地址", heading2_style))
    story.append(Paragraph("<link href='https://fuxioffice.cn/'>https://fuxioffice.cn/</link>", body_style))

    story.append(Paragraph("演示账号", heading2_style))
    demo_data = [
        ['角色', '用户名', '密码'],
        ['管理员', 'admin', 'admin1234'],
        ['普通用户', 'fuxi', 'admin1234'],
    ]
    demo_table = Table(demo_data, colWidths=[4*cm, 5*cm, 5*cm])
    demo_table.setStyle(TableStyle([
        ('BACKGROUND', (0, 0), (-1, 0), colors.HexColor('#0066cc')),
        ('TEXTCOLOR', (0, 0), (-1, 0), colors.whitesmoke),
        ('ALIGN', (0, 0), (-1, -1), 'LEFT'),
        ('FONTNAME', (0, 0), (-1, 0), 'Helvetica-Bold'),
        ('FONTSIZE', (0, 0), (-1, -1), 9),
        ('GRID', (0, 0), (-1, -1), 1, colors.grey),
        ('ROWBACKGROUNDS', (0, 1), (-1, -1), [colors.white, colors.HexColor('#f9f9f9')]),
    ]))
    story.append(demo_table)
    story.append(Spacer(1, 0.3*cm))
    story.append(Paragraph("<i>以上为公开演示账号；正式部署时请立即修改初始密码。</i>",
                          ParagraphStyle('note', parent=body_style, fontSize=9, textColor=colors.grey)))

    story.append(Spacer(1, 0.5*cm))
    story.append(Paragraph("推荐演示路径", heading2_style))
    demo_steps = [
        '进入工作台：打开线上地址并登录，进入项目空间',
        '多智能体协作建模：输入贷款审批需求，观察 Analyst、Architect、Engineer 多角色并行工作',
        'RAG 知识增强：在知识库中添加术语定义，观察建模时自动注入相关上下文',
        '决策建模与验收：确认方案后生成 DSL 并编译为 DMN，执行测试用例后发布',
        '运行与解释：调用已发布决策服务，查看输出、执行轨迹，按需生成决策解释',
        '偏差归因与优化：回传业务结果，查看智能归因候选，查看优化建议并执行 Shadow 评估',
        '漂移检测与 Copilot：配置漂移规则，使用 Copilot 自然语言查询项目指标',
        '增量修改与审查：提出"将风险阈值从 0.55 调整为 0.50"，对比变更审查',
    ]
    for i, step in enumerate(demo_steps, 1):
        story.append(Paragraph(f"<b>{i}.</b> {step}", bullet_style))

    story.append(PageBreak())

    # 六、开源项目价值
    story.append(Paragraph("六、开源项目价值", heading1_style))

    values = [
        ("<b>对业务人员</b>", "用业务语言建模，减少学习 DMN、XML 和 FEEL 的成本；通过决策解释理解决策逻辑"),
        ("<b>对研发团队</b>", "结构化 DSL、编译器和测试门禁降低规则代码维护成本；多智能体协作提升建模效率"),
        ("<b>对管理与合规</b>", "版本、审批、审计、回滚和执行轨迹形成治理闭环；偏差归因与漂移检测提升运营质量"),
        ("<b>对平台建设</b>", "Rust 单体二进制易部署，领域模块清晰，便于后续拆分服务"),
        ('<b>对 AI 应用研究</b>', '提供"多智能体协作 + RAG 检索 + 确定性运行时 + 人在回路 + 完整闭环"的可落地样例'),
    ]
    for title, desc in values:
        story.append(Paragraph(f"{title}: {desc}", bullet_style))

    story.append(Spacer(1, 0.8*cm))

    # 结语
    story.append(Paragraph("结语", heading1_style))
    story.append(Paragraph(
        '伏羲不是让 AI 替代规则系统，而是让 AI 成为规则系统的自然语言建模入口。通过"多智能体协作、RAG 检索、DSL 表达、编译器校验、确定性执行、人工治理、运行反馈、偏差归因、漂移检测、优化闭环"的组合，企业可以把分散在文档和经验中的决策知识，逐步沉淀为可执行、可解释、可审计的数字化资产。',
        body_style))

    story.append(Spacer(1, 0.3*cm))
    story.append(Paragraph(
        "<b>v0.2.0 实现了从 L2（单点辅助）到 L3（半自动闭环）的重大升级</b>，形成了完整的智能化决策自动化闭环。",
        body_style))

    story.append(Spacer(1, 1*cm))
    story.append(Paragraph(
        "线上演示：https://fuxioffice.cn/",
        ParagraphStyle('footer', parent=body_style, alignment=TA_CENTER, fontSize=10)))
    story.append(Paragraph(
        "文档版本：v0.2.0 · 2026-11-18",
        ParagraphStyle('footer2', parent=body_style, alignment=TA_CENTER, fontSize=9, textColor=colors.grey)))

    # 构建 PDF
    doc.build(story)
    print(f"✅ PDF 已生成: {output_file}")

if __name__ == '__main__':
    script_dir = Path(__file__).parent
    project_root = script_dir.parent
    output_pdf = project_root / 'docs' / 'project-introduction.pdf'

    try:
        create_pdf(output_pdf)
        print(f"📄 PDF 文件大小: {output_pdf.stat().st_size / 1024:.1f} KB")
    except Exception as e:
        print(f"❌ 生成 PDF 失败: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)
