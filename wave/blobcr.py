import subprocess, sys
repo, base = sys.argv[1], sys.argv[2]
files = subprocess.run(["git", "-C", repo, "diff", "--name-only", base, "HEAD"], capture_output=True, text=True).stdout.split()
bad = [f for f in files if b"\r" in subprocess.run(["git", "-C", repo, "cat-file", "-p", "HEAD:" + f], capture_output=True).stdout]
print(len(files), "files;", len(bad), "with CR", bad[:5])
