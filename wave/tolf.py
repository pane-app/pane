import sys
root, listfile = sys.argv[1], sys.argv[2]
n = 0
for rel in open(listfile, encoding="utf-8").read().split():
    p = root + "/" + rel
    b = open(p, "rb").read()
    nb = b.replace(b"\r", b"")
    if nb != b:
        open(p, "wb").write(nb)
        n += 1
print("normalized", n)
