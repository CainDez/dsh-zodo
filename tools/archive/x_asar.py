import json, struct, sys
from pathlib import Path
p = Path(r"C:\Users\<user>\AppData\Local\Programs\DeepSeek Harness\resources\app.asar")
out = Path(r"D:\projects\zodo_dsh\.runtime\asar-x")
with p.open('rb') as f:
    magic, h1, h2, jlen = struct.unpack('<IIII', f.read(16))
    root = json.loads(f.read(jlen))
data_start = 16 + jlen
files = {}
def walk(node, prefix):
    for name, v in node.get('files', {}).items():
        path = f"{prefix}/{name}" if prefix else name
        if 'files' in v:
            walk(v, path)
        else:
            files[path] = (int(v.get('offset',0)), int(v.get('size',0)), bool(v.get('unpacked', False)))
walk(root, '')
cmd = sys.argv[1]
if cmd == 'list':
    import re
    pat = re.compile(sys.argv[2], re.I)
    hits = [x for x in files if pat.search(x)]
    for h in sorted(hits):
        print(h, files[h][1])
    print('TOTAL', len(hits), 'of', len(files), file=sys.stderr)
elif cmd == 'extract':
    pref = sys.argv[2]
    n = 0
    with p.open('rb') as f:
        for path,(off,size,unpacked) in files.items():
            if path.startswith(pref) and not unpacked:
                dest = out / path
                dest.parent.mkdir(parents=True, exist_ok=True)
                f.seek(data_start+off)
                dest.write_bytes(f.read(size))
                n += 1
    print('extracted', n, 'to', out)
