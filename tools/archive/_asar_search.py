import json, struct, sys
from pathlib import Path

p = Path(r"C:\Users\<user>\AppData\Local\Programs\DeepSeek Harness\resources\app.asar")
with p.open('rb') as f:
    head = f.read(16)
    magic, h1, h2, jlen = struct.unpack('<IIII', head)
    js = f.read(jlen)
    root = json.loads(js)
    data_start = 16 + jlen

files = []
def walk(node, prefix):
    for name, v in node.get('files', {}).items():
        path = f"{prefix}/{name}" if prefix else name
        if 'files' in v:
            walk(v, path)
        else:
            files.append((path, int(v.get('offset', 0)), int(v.get('size', 0))))
walk(root, '')
files.sort(key=lambda x: x[1])
print('files', len(files), 'data_start', data_start)
# search raw bytes
needles = [b'managed exclusively by the Electron application', b'plugin-manager', b'plugin_manager', b'plugin install']
with p.open('rb') as f:
    f.seek(data_start)
    raw = f.read()
for needle in needles:
    print('\nNEEDLE', needle)
    pos = 0
    count = 0
    while count < 20:
        i = raw.find(needle, pos)
        if i < 0: break
        count += 1
        abspos = data_start + i
        # find file
        import bisect
        idx = bisect.bisect_right([x[1] for x in files], i) - 1
        # handle gaps? use index
        cand = files[max(0,idx)]
        print('offset', i, 'file', cand[0], 'ctx', raw[max(0,i-150):i+250].decode('utf-8','replace').replace('\n',' ')[:500])
        pos = i + len(needle)
    print('count', count)
