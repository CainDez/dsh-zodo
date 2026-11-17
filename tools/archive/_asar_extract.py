import json, struct, sys
from pathlib import Path

p = Path(r"C:\Users\<user>\AppData\Local\Programs\DeepSeek Harness\resources\app.asar")
out = Path(r"D:\projects\zodo_dsh\tools\_asar_extract")
out.mkdir(parents=True, exist_ok=True)
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
                files[path] = (int(v.get('offset', 0)), int(v.get('size', 0)), v.get('unpacked', False))
    walk(root, '')
    print('files', len(files))
    wanted = [x for x in files if 'plugin-manager' in x or 'plugin-manager' in x or 'dsh-plugin-manager' in x or 'plugin-manager' in x]
    # add all files inside these dirs plus CLI bin
    wanted = sorted(wanted)
    for path in wanted:
        print(path, files[path])
    # extract specific dirs
    prefixes = [
        'dsh/node_modules/@deepseek-ai/dsh-plugin-manager/',
        'dsh/node_modules/@deepseek-ai/dsh-client-ui-plugin-manager/',
        'dsh/node_modules/@deepseek-ai/dsh-plugin-detail/',
        'dsh/node_modules/@deepseek-ai/dsh-plugin-market/',
    ]
    for path, (off, size, unpacked) in files.items():
        if any(path.startswith(x) for x in prefixes) and not unpacked:
            dest = out / path
            dest.parent.mkdir(parents=True, exist_ok=True)
            f.seek(data_start + off)
            dest.write_bytes(f.read(size))
    # extract CLI bin.js
    for path in ['dsh/node_modules/@deepseek-ai/dsh/lib/bin.js']:
        off, size, unpacked = files[path]
        dest = out / path
        dest.parent.mkdir(parents=True, exist_ok=True)
        f.seek(data_start + off)
        dest.write_bytes(f.read(size))
    print('done', out)
