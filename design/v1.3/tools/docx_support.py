from __future__ import annotations
import re
from docx import Document
from docx.enum.table import WD_CELL_VERTICAL_ALIGNMENT
from docx.oxml import OxmlElement
from docx.oxml.ns import qn
from docx.shared import Cm, Pt, RGBColor
def set_font(run, size=None, bold=None, name='Microsoft YaHei'):
    run.font.name = name
    if size is not None:
        run.font.size = Pt(size)
    if bold is not None:
        run.bold = bold
    props = run._element.get_or_add_rPr()
    fonts = props.find(qn('w:rFonts'))
    if fonts is None:
        fonts = OxmlElement('w:rFonts')
        props.append(fonts)
    for attr in ['ascii', 'hAnsi', 'eastAsia', 'cs']:
        fonts.set(qn('w:'+attr), name)

def add_hyperlink(paragraph, label, url):
    rel = paragraph.part.relate_to(url, 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink', is_external=True)
    link = OxmlElement('w:hyperlink')
    link.set(qn('r:id'),rel)
    run = OxmlElement('w:r')
    props = OxmlElement('w:rPr')
    col = OxmlElement('w:color'); col.set(qn('w:val'),'244C72'); props.append(col)
    fonts = OxmlElement('w:rFonts'); fonts.set(qn('w:eastAsia'),'Microsoft YaHei'); props.append(fonts)
    run.append(props)
    text = OxmlElement('w:t'); text.text=label; run.append(text)
    link.append(run); paragraph._p.append(link)

def inline(paragraph, text, size=None):
    text = text.replace('`','')
    pattern = re.compile(r'\[([^\]]+)\]\(([^)]+)\)|\*\*([^*]+)\*\*')
    pos=0
    for match in pattern.finditer(text):
        if match.start()>pos:
            set_font(paragraph.add_run(text[pos:match.start()]),size)
        if match.group(1):
            add_hyperlink(paragraph,match.group(1),match.group(2))
        else:
            set_font(paragraph.add_run(match.group(3)),size,True)
        pos=match.end()
    if pos<len(text):
        set_font(paragraph.add_run(text[pos:]),size)

def plain_heading(text):
    text = re.sub(r'\[([^\]]+)\]\([^)]+\)',r'\1',text)
    text = re.sub(r'[\W_]+',' ',text,flags=re.UNICODE)
    return re.sub(r'\s+',' ',text).strip()

def table(document, rows):
    rows=[r for r in rows if not all(re.fullmatch(r':?-{2,}:?',c.strip()) for c in r)]
    count=max(map(len,rows))
    t=document.add_table(rows=0,cols=count)
    t.autofit=False
    proportions={2:[.27,.73],3:[.21,.48,.31],4:[.17,.26,.33,.24]}.get(count,[1/count]*count)
    for c,p in zip(t.columns,proportions):
        c.width=Cm(17*p)
    borders=OxmlElement('w:tblBorders')
    for edge in ['top','left','bottom','right','insideH','insideV']:
        e=OxmlElement('w:'+edge);e.set(qn('w:val'),'single');e.set(qn('w:sz'),'4');e.set(qn('w:color'),'D9D9D9');borders.append(e)
    t._tbl.tblPr.append(borders)
    for ridx,values in enumerate(rows):
        cells=t.add_row().cells
        cant_split=OxmlElement('w:cantSplit')
        t.rows[-1]._tr.get_or_add_trPr().append(cant_split)
        for index,cell in enumerate(cells):
            cell.width=Cm(17*proportions[index]);cell.vertical_alignment=WD_CELL_VERTICAL_ALIGNMENT.CENTER
            prop=cell._tc.get_or_add_tcPr()
            shade=OxmlElement('w:shd');shade.set(qn('w:fill'),'193647' if ridx==0 else ('F5F7F9' if ridx%2==0 else 'FFFFFF'));prop.append(shade)
            margins=OxmlElement('w:tcMar')
            for side,value in [('top',75),('bottom',75),('left',105),('right',105)]:
                e=OxmlElement('w:'+side);e.set(qn('w:w'),str(value));e.set(qn('w:type'),'dxa');margins.append(e)
            prop.append(margins)
            p=cell.paragraphs[0];p.paragraph_format.space_after=Pt(1);p.paragraph_format.line_spacing=1.15
            inline(p,values[index] if index<len(values) else '',9.5)
            if ridx==0:
                p.paragraph_format.keep_with_next=True
                for run in p.runs:
                    run.font.color.rgb=RGBColor(255,255,255);run.bold=True
        if ridx==0:
            repeat=OxmlElement('w:tblHeader');t.rows[0]._tr.get_or_add_trPr().append(repeat)
    document.add_paragraph().paragraph_format.space_after=Pt(2)

def markdown(document, source):
    lines=source.splitlines()
    index=0
    while index<len(lines):
        line=lines[index].strip()
        if not line or line.startswith('# '):
            index+=1;continue
        if line.startswith(('~~~','```')):
            index+=1
            code_paragraphs=[]
            while index<len(lines) and not lines[index].strip().startswith(('~~~','```')):
                p=document.add_paragraph(style='Code')
                set_font(p.add_run(lines[index]),8.6,name='Consolas')
                p.paragraph_format.keep_together=True
                code_paragraphs.append(p)
                index+=1
            for p in code_paragraphs[:-1]:
                p.paragraph_format.keep_with_next=True
            index+=1;continue
        if line.startswith('|'):
            rows=[]
            while index<len(lines) and lines[index].strip().startswith('|'):
                rows.append([v.strip() for v in lines[index].strip().strip('|').split('|')]);index+=1
            table(document,rows);continue
        if line.startswith('##'):
            level=min(3,len(line)-len(line.lstrip('#')))
            h=document.add_heading(plain_heading(line.lstrip('#').strip()),level=level)
            index+=1;continue
        if line.startswith('>'):
            p=document.add_paragraph();inline(p,line.lstrip('> ').strip(),10);index+=1;continue
        if line.startswith('- '):
            p=document.add_paragraph(style='List Bullet');inline(p,line[2:]);index+=1;continue
        block=[line];index+=1
        while index<len(lines) and lines[index].strip() and not lines[index].strip().startswith(('#','|','>','- ','~~~','```')) and not re.match(r'^\d+\.',lines[index].strip()):
            block.append(lines[index].strip());index+=1
        p=document.add_paragraph();inline(p,' '.join(block))

def configure(document):
    section=document.sections[0]
    section.page_width=Cm(21);section.page_height=Cm(29.7)
    section.top_margin=Cm(1.8);section.bottom_margin=Cm(1.8)
    section.left_margin=Cm(2);section.right_margin=Cm(2)
    section.header_distance=Cm(.8);section.footer_distance=Cm(.8)
    normal=document.styles['Normal']
    normal.font.name='Microsoft YaHei';normal.font.size=Pt(10.5)
    normal._element.rPr.rFonts.set(qn('w:eastAsia'),'Microsoft YaHei')
    normal.paragraph_format.line_spacing=1.35
    normal.paragraph_format.space_after=Pt(6)
    normal.paragraph_format.widow_control=True
    snap=OxmlElement('w:snapToGrid');snap.set(qn('w:val'),'0');normal._element.get_or_add_pPr().append(snap)
    for name,size in [('Title',26),('Subtitle',13),('Heading 1',18),('Heading 2',13),('Heading 3',11.5)]:
        s=document.styles[name];s.font.name='Microsoft YaHei';s.font.size=Pt(size);s.font.color.rgb=RGBColor(0,0,0)
        s._element.rPr.rFonts.set(qn('w:eastAsia'),'Microsoft YaHei')
        s.font.italic=False
        s.paragraph_format.space_before=Pt(12 if name.startswith('Heading') else 3)
        s.paragraph_format.space_after=Pt(7)
        s.paragraph_format.keep_with_next=True
        props=s._element.get_or_add_pPr()
        snap=OxmlElement('w:snapToGrid');snap.set(qn('w:val'),'0');props.append(snap)
        for b in list(props.findall(qn('w:pBdr'))): props.remove(b)
    style=document.styles.add_style('Code',1)
    style.font.name='Consolas';style.font.size=Pt(8.6)
    style.paragraph_format.line_spacing=1.0
    style.paragraph_format.space_after=Pt(0)
    style.paragraph_format.left_indent=Cm(.25)
    snap=OxmlElement('w:snapToGrid');snap.set(qn('w:val'),'0');style._element.get_or_add_pPr().append(snap)
    header=section.header.paragraphs[0]
    header.paragraph_format.space_after=Pt(0)
    set_font(header.add_run('Zodo 软件设计方案'),8.5)
    footer=section.footer.paragraphs[0]
    footer.alignment=2
    set_font(footer.add_run('实施基线 1.3    第 '),8.5)
    field=OxmlElement('w:fldSimple');field.set(qn('w:instr'),'PAGE');footer._p.append(field)
    set_font(footer.add_run(' 页'),8.5)
