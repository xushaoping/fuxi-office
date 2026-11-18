#!/usr/bin/env python3
"""
生成伏羲智能决策平台产品介绍 PDF
使用 markdown 转 HTML，然后可以用浏览器打印为 PDF
"""

import markdown
import sys
from pathlib import Path

def generate_html_from_markdown(md_file, output_html):
    """将 Markdown 文件转换为格式良好的 HTML"""

    # 读取 Markdown 内容
    with open(md_file, 'r', encoding='utf-8') as f:
        md_content = f.read()

    # 转换 Markdown 到 HTML
    html_content = markdown.markdown(
        md_content,
        extensions=['extra', 'codehilite', 'tables', 'toc']
    )

    # HTML 模板，带样式
    html_template = f"""<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>伏羲智能决策平台 v1.0.0 - 产品介绍</title>
    <style>
        @page {{
            size: A4;
            margin: 2cm;
        }}

        body {{
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", "Helvetica Neue", Arial, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 900px;
            margin: 0 auto;
            padding: 20px;
            font-size: 14px;
        }}

        h1 {{
            color: #1a1a1a;
            border-bottom: 3px solid #0066cc;
            padding-bottom: 10px;
            margin-top: 30px;
            font-size: 32px;
        }}

        h2 {{
            color: #0066cc;
            margin-top: 25px;
            font-size: 24px;
            border-bottom: 2px solid #e0e0e0;
            padding-bottom: 8px;
        }}

        h3 {{
            color: #0088cc;
            margin-top: 20px;
            font-size: 18px;
        }}

        h4 {{
            color: #555;
            margin-top: 15px;
            font-size: 16px;
        }}

        p {{
            margin: 10px 0;
            text-align: justify;
        }}

        ul, ol {{
            margin: 10px 0;
            padding-left: 30px;
        }}

        li {{
            margin: 5px 0;
        }}

        strong {{
            color: #000;
            font-weight: 600;
        }}

        code {{
            background-color: #f5f5f5;
            padding: 2px 6px;
            border-radius: 3px;
            font-family: "Consolas", "Monaco", "Courier New", monospace;
            font-size: 13px;
        }}

        pre {{
            background-color: #f8f8f8;
            border: 1px solid #ddd;
            border-radius: 5px;
            padding: 15px;
            overflow-x: auto;
            font-size: 13px;
        }}

        pre code {{
            background-color: transparent;
            padding: 0;
        }}

        table {{
            border-collapse: collapse;
            width: 100%;
            margin: 15px 0;
            font-size: 13px;
        }}

        th, td {{
            border: 1px solid #ddd;
            padding: 10px;
            text-align: left;
        }}

        th {{
            background-color: #0066cc;
            color: white;
            font-weight: 600;
        }}

        tr:nth-child(even) {{
            background-color: #f9f9f9;
        }}

        hr {{
            border: none;
            border-top: 2px solid #e0e0e0;
            margin: 30px 0;
        }}

        blockquote {{
            border-left: 4px solid #0066cc;
            padding-left: 15px;
            margin-left: 0;
            color: #555;
            font-style: italic;
        }}

        .emoji {{
            font-size: 1.2em;
        }}

        /* 打印样式 */
        @media print {{
            body {{
                font-size: 12px;
            }}

            h1 {{
                font-size: 28px;
                page-break-before: always;
            }}

            h1:first-of-type {{
                page-break-before: avoid;
            }}

            h2 {{
                font-size: 20px;
                page-break-after: avoid;
            }}

            h3 {{
                font-size: 16px;
                page-break-after: avoid;
            }}

            table, pre {{
                page-break-inside: avoid;
            }}

            a {{
                color: #0066cc;
                text-decoration: none;
            }}
        }}

        /* 封面样式 */
        .cover {{
            text-align: center;
            padding: 100px 0;
            page-break-after: always;
        }}

        .cover h1 {{
            font-size: 48px;
            color: #0066cc;
            border: none;
            margin-bottom: 20px;
        }}

        .cover .subtitle {{
            font-size: 20px;
            color: #666;
            margin: 20px 0;
        }}

        .cover .version {{
            font-size: 16px;
            color: #999;
            margin-top: 50px;
        }}
    </style>
</head>
<body>
    <div class="cover">
        <h1>伏羲智能决策平台</h1>
        <p class="subtitle">OPEN SOURCE · AI DECISION AUTOMATION</p>
        <p class="subtitle">让业务规则从自然语言出发，经过多智能体协作、结构化建模、<br>确定性校验与人工治理，成为可执行、可解释、可审计的决策服务</p>
        <p class="version">版本 v0.2.0 · 2026-11-18</p>
    </div>

    {html_content}

    <hr>
    <footer style="text-align: center; color: #999; font-size: 12px; margin-top: 50px;">
        <p>线上演示：<a href="https://fuxioffice.cn/">https://fuxioffice.cn/</a></p>
        <p>文档版本：v0.2.0 · 2026-11-18</p>
    </footer>
</body>
</html>
"""

    # 写入 HTML 文件
    with open(output_html, 'w', encoding='utf-8') as f:
        f.write(html_template)

    print(f"✅ HTML 已生成: {output_html}")
    print(f"📄 请使用浏览器打开并打印为 PDF：")
    print(f"   1. 在浏览器中打开: {output_html}")
    print(f"   2. 按 Cmd+P（Mac）或 Ctrl+P（Windows）")
    print(f"   3. 选择'另存为 PDF'")
    print(f"   4. 保存为: project-introduction.pdf")

if __name__ == '__main__':
    script_dir = Path(__file__).parent
    project_root = script_dir.parent
    md_file = project_root / 'docs' / 'project-introduction-v0.2.0.md'
    output_html = project_root / 'docs' / 'project-introduction-v0.2.0.html'

    if not md_file.exists():
        print(f"❌ 找不到 Markdown 文件: {md_file}")
        sys.exit(1)

    generate_html_from_markdown(md_file, output_html)
