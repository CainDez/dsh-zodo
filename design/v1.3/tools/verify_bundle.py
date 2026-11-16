"""Verify an unchanged design handoff using only the Python standard library."""
from __future__ import annotations
import csv,hashlib,json,re,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def main():
    manifest=json.loads((ROOT/'MANIFEST.json').read_text(encoding='utf-8'))
    for item in manifest['files']:
        path=(ROOT/item['path']).resolve()
        assert path.is_relative_to(ROOT.resolve()),item['path']
        assert path.is_file() and path.stat().st_size==item['bytes'] and sha(path)==item['sha256'],item['path']
    for line in (ROOT/'SHA256SUMS.txt').read_text(encoding='utf-8').splitlines():
        expected,name=line.split('  ',1)
        assert sha(ROOT/name)==expected,name
    broken=[]
    for md in ROOT.rglob('*.md'):
        for target in re.findall(r'\]\(([^)]+)\)',md.read_text(encoding='utf-8')):
            target=target.strip('<>').split('#')[0]
            if not target or re.match(r'^[a-zA-Z]+://',target) or target.startswith('mailto:'):continue
            if re.match(r'^[A-Za-z]:[/\\]',target) or target.startswith('/'):
                broken.append([md.relative_to(ROOT).as_posix(),'nonportable '+target]);continue
            if not (md.parent/target).resolve().is_file():broken.append([str(md.relative_to(ROOT)),target])
    assert not broken,broken
    for path in ROOT.rglob('*.json'):json.loads(path.read_text(encoding='utf-8'))
    visuals=json.loads((ROOT/'prototype/visual-manifest.json').read_text(encoding='utf-8'))
    assert visuals['selected_direction']=='clear-day' and len(visuals['files'])==7
    for item in visuals['files']:assert sha(ROOT/'prototype'/item['file'])==item['sha256']
    demo=json.loads((ROOT/'fixtures/demo-workspace.json').read_text(encoding='utf-8'))
    assert sum(x['estimate_minutes'] for x in demo['work_segments'])==240
    assert demo['review']['actual_minutes']+demo['review']['remaining_minutes']==90
    with (ROOT/'implementation/ACCEPTANCE_MATRIX.csv').open(encoding='utf-8-sig',newline='') as f:rows=list(csv.DictReader(f))
    assert len(rows)==len({x['screen_id'] for x in rows})==30
    status=json.loads((ROOT/'BUNDLE_STATUS.json').read_text(encoding='utf-8'))
    assert status['clickable_flutter_prototype'] is False and status['rust_kernel_implemented'] is False
    assert status['first_ui']=='deepseek-harness-plugin' and not status['harness_integration_tested']
    with (ROOT/'implementation/HARNESS_TEST_MATRIX.csv').open(encoding='utf-8-sig',newline='') as stream: host_rows=list(csv.DictReader(stream))
    assert len(host_rows)==len({row['test_id'] for row in host_rows})==25
    assert {row['status'] for row in host_rows}=={'未执行'}
    assert (ROOT/'spec/06-harness-adapter-and-ui.md').is_file() and (ROOT/'contracts/HARNESS_BRIDGE.md').is_file()
    review=json.loads((ROOT/'qa/document-review.json').read_text(encoding='utf-8'))
    assert review['manual_visual_review']=='passed' and status['document_render_review']=='passed'
    assert len(review['documents'])==3
    for doc in review['documents']:
        assert sha(ROOT/doc['file'])==doc['sha256'],doc['file']
        assert len(doc['page_reviews'])==doc['pages']
        assert all(page['manual_review']=='passed' for page in doc['page_reviews']),doc['file']
    print(json.dumps({'files_verified':len(manifest['files']),'selection':'clear-day','first_ui':status['first_ui'],'visuals':7,'screens':30,'host_test_cases':len(host_rows),'document_pages':{doc['key']:doc['pages'] for doc in review['documents']},'local_links':'passed','json':'passed','hashes':'passed','product_implemented':False,'product_runtime_tested':False},ensure_ascii=False))
if __name__=='__main__':main()
