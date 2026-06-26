import re, hashlib

idx = "src/treesitter/_index.yaml"
s = open(idx).read()
for path, asset in [("treesitter.wasm", "src/treesitter/assets/treesitter.wasm")]:
    h = hashlib.sha256(open(asset, "rb").read()).hexdigest()
    pat = re.compile(r"(path:\s*" + re.escape(path) + r"\s*\n(?:.*\n)*?\s*hash:\s*sha256:)[0-9a-f]+", re.M)
    s = pat.sub(lambda m: m.group(1) + h, s, count=0)
    print("staged", path, h[:16])
open(idx, "w").write(s)
