from __future__ import annotations
import json,re
from pathlib import Path
from docx import Document
from docx.enum.section import WD_ORIENT
from docx.enum.text import WD_ALIGN_PARAGRAPH
from docx.shared import Inches, Pt, RGBColor
from docx.oxml import OxmlElement
from docx.oxml.ns import qn
from docx_support import configure, markdown, set_font

ROOT=Path(__file__).resolve().parents[1]
def page_number(doc):
    p=doc.sections[0].footer.paragraphs[0]
    p.alignment=WD_ALIGN_PARAGRAPH.CENTER
    set_font(p.add_run('清晰日程设计基线 1.3    第 '),8.5)
    field=OxmlElement('w:fldSimple');field.set(qn('w:instr'),'PAGE');p._p.append(field)
    set_font(p.add_run(' 页'),8.5)

def software():
    doc=Document();configure(doc)
    doc.core_properties.title='Zodo 软件设计方案'
    doc.add_paragraph('Zodo 软件设计方案',style='Title')
    doc.add_paragraph('Rust 独立 AI 内核与 Harness 首期插件',style='Subtitle')
    doc.add_paragraph('版本 1.3    2026年10月24日    Harness 首期路线已确认')
    doc.add_paragraph('本包指导在新项目中独立重做个人事务 AI 产品。内核负责事实、时间、约束、权限与提交，Harness、CLI、MCP 和后续 Flutter 共享同一应用能力。本地和云端模型按数据策略接入。')
    doc.add_paragraph('用户已确认Harness插件先行，Rust内核目标和全功能范围不变，Flutter后续接入。七张旧清晰日程图仅为视觉参考；Harness新界面、插件、Rust内核、真实模型和平台运行尚未实现。专章及桥接契约与A0–A7计划构成当前实施基线。')
    doc.add_picture(str(ROOT/'assets/architecture-overview.png'),width=Inches(6.45))
    doc.paragraphs[-1].paragraph_format.line_spacing=1
    doc.add_paragraph('实施入口：START_HERE.md 与 IMPLEMENTATION_PROMPT.md。实施状态以 BUNDLE_STATUS.json 和逐屏验收表记录。')
    specs=sorted((ROOT/'spec').glob('*.md'))
    for index,file in enumerate(specs):
        text=file.read_text(encoding='utf-8')
        if index==0:doc.add_page_break()
        h=doc.add_heading(text.splitlines()[0].lstrip('# '),1)
        if index:h.paragraph_format.space_before=Pt(20)
        markdown(doc,re.sub(r"```mermaid[\s\S]*?```", "关系图见本册封面和Markdown原文。", text))
    path=ROOT/'documents/Zodo软件设计方案.docx'
    path.parent.mkdir(parents=True,exist_ok=True);doc.save(path)
    return path

def prototype():
    visuals=json.loads((ROOT/'prototype/visual-manifest.json').read_text(encoding='utf-8'))['files']
    doc=Document();section=doc.sections[0]
    section.orientation=WD_ORIENT.LANDSCAPE
    section.page_width=Inches(11);section.page_height=Inches(8.5)
    section.top_margin=section.bottom_margin=Inches(.5)
    section.left_margin=section.right_margin=Inches(.5)
    section.footer_distance=Inches(.18)
    for name,size in [('Normal',11),('Title',26),('Heading 1',17)]:
        s=doc.styles[name];s.font.name='Microsoft YaHei';s.font.size=Pt(size);s.font.color.rgb=RGBColor(0,0,0)
        s._element.get_or_add_rPr().rFonts.set(qn('w:eastAsia'),'Microsoft YaHei')
        s.paragraph_format.line_spacing=1;s.paragraph_format.space_after=Pt(5)
        s.paragraph_format.keep_with_next=name!='Normal'
        ppr=s._element.get_or_add_pPr()
        for border in list(ppr.findall(qn('w:pBdr'))):ppr.remove(border)
    page_number(doc);doc.core_properties.title='清晰日程视觉参考册'
    doc.add_paragraph('清晰日程视觉参考册',style='Title')
    doc.add_paragraph('旧布局视觉参考    首期采用 Harness 插件    版本 1.3    2026年10月24日')
    doc.add_paragraph('现有六个桌面页与一个手机页提供清晰日程的视觉和信息层级参考，均来自先前独立布局。首期必须按Harness宿主Slots、工作台和权限重新适配；本册未提供Harness新截图。完整文字规格仍覆盖30个屏幕族、16类弹层与八条核心流程。')
    doc.add_paragraph('这是静态参考册，图内按钮不能点击；Harness插件、Rust内核与后续Flutter尚未实现。五业务入口在插件内部组织，手机底栏与原生后台规则归后续客户端，不直接搬成宿主全局布局。图是位图，标题与说明是可编辑文字。')
    doc.add_paragraph('日程和执行内容均为构造数据。Today、Inbox、Review分别参考上午09:20、下午16:30和17:00。下周改期与今日手机提案是不同候选场景，不是一次真实排程。')
    doc.add_paragraph('D01图中清单为两项，实施时介绍文案按ui/COPY.md修正为两项。手机只接受邮件一项需形成当前单项提案，不能复用旧批准。')
    h=doc.add_heading('视觉参考索引与后续适配',1);h.paragraph_format.page_break_before=True
    table=doc.add_table(rows=1,cols=4);table.autofit=False
    widths=[.6,2.8,4.7,1.0]
    for col,width in zip(table.columns,widths):col.width=Inches(width)
    for c,t in zip(table.rows[0].cells,['编号','画面','实施目的','页码']):c.text=t
    purposes=['下一步与承诺，待确认建议','原文、字段与提醒澄清','结果标准、里程碑与估时','议程差异与当前批准','事实、剩余量与调整建议','本地/云路由、数据与助理范围','独立手机单项确认']
    for index,(item,purpose) in enumerate(zip(visuals,purposes),3):
        for c,t in zip(table.add_row().cells,[item['screen'],item['title'],purpose,str(index)]):c.text=t
    borders=OxmlElement('w:tblBorders')
    for edge in ['top','left','bottom','right','insideH','insideV']:
        b=OxmlElement('w:'+edge);b.set(qn('w:val'),'single');b.set(qn('w:sz'),'4');b.set(qn('w:color'),'D9D9D9');borders.append(b)
    table._tbl.tblPr.append(borders)
    for rowindex,row in enumerate(table.rows):
        for cell in row.cells:
            shd=OxmlElement('w:shd');shd.set(qn('w:fill'),'E9ECEF' if rowindex==0 else 'FFFFFF');cell._tc.get_or_add_tcPr().append(shd)
            for p in cell.paragraphs:
                p.paragraph_format.space_after=Pt(5);p.paragraph_format.space_before=Pt(5)
                for run in p.runs:set_font(run,10.5,rowindex==0)
    doc.add_paragraph('文字与行为：ui/01-screen-and-flow-spec.md、ui/02-ai-and-settings-flows.md。颜色与组件：prototype/design-tokens.json、ui/03-visual-spec.md。')
    for item in visuals:
        h=doc.add_heading('清晰日程  '+item['screen']+'  '+item['title'],1);h.paragraph_format.page_break_before=True
        p=doc.add_paragraph('旧独立布局参考；首期按Harness公开UI载体重新适配。本图不计Harness新页面、截图或交互完成。')
        p.paragraph_format.keep_with_next=True
        p=doc.add_paragraph();p.alignment=WD_ALIGN_PARAGRAPH.CENTER
        p.paragraph_format.line_spacing=1;p.paragraph_format.space_before=Pt(0);p.paragraph_format.space_after=Pt(0)
        run=p.add_run()
        if item['surface']=='mobile':run.add_picture(str(ROOT/'prototype'/item['file']),height=Inches(6.25))
        else:run.add_picture(str(ROOT/'prototype'/item['file']),width=Inches(8.65))
    path=ROOT/'documents/清晰日程视觉参考册.docx';doc.save(path)
    assert len(doc.inline_shapes)==7
    return path


def plan():
    doc=Document();configure(doc)
    header=doc.sections[0].header.paragraphs[0];header.clear();set_font(header.add_run('Zodo 持续开发计划'),8.5)
    doc.core_properties.title='Zodo 持续开发计划'
    doc.add_paragraph('Zodo 持续开发计划',style='Title')
    doc.add_paragraph('Harness 首期插件与独立 Rust 内核',style='Subtitle')
    doc.add_paragraph('版本 1.3    2026年10月24日    完整范围 P0 P1 P2 与后续 Flutter')
    doc.add_paragraph('本计划指导新项目持续实施、自测、修复并进入下一阶段。A0先验证宿主树外插件能力，随后完成Rust内核与Harness闭环，再推进P1、Flutter、P2及全量回归。阶段或模拟通过不能代替完整目标完成。')
    source=(ROOT/'implementation/PLAN.md').read_text(encoding='utf-8')
    markdown(doc,source)
    doc.add_heading('持续开发提示词入口',1)
    doc.add_paragraph('IMPLEMENTATION_PROMPT.md可直接复制到新项目。它规定功能覆盖账本、自动阶段推进、失败修复、真实宿主/模型/平台证据、上下文检查点与外部阻塞处理。此资料未执行该开发任务。')
    path=ROOT/'documents/Zodo持续开发计划.docx';doc.save(path)
    return path

if __name__=='__main__':
    paths=[software(),plan(),prototype()]
    print(json.dumps({'created':[str(p) for p in paths],'prototype_expected_pages':9,'product_code_created':False},ensure_ascii=False))
