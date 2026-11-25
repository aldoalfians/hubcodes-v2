#!/usr/bin/env python3
"""Render a tmux pane (with ANSI colours) to PNG via headless Chrome.

Usage: tmux2png.py <tmux-target> <out.png> [title]
Requires: tmux, Google Chrome (macOS path below) — nothing else.
"""
import html
import os
import re
import subprocess
import sys
import tempfile

CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

# xterm 256-colour palette.
def palette():
    base = [
        "000000", "cd0000", "00cd00", "cdcd00", "0000ee", "cd00cd", "00cdcd", "e5e5e5",
        "7f7f7f", "ff0000", "00ff00", "ffff00", "5c5cff", "ff00ff", "00ffff", "ffffff",
    ]
    cols = [int(c, 16) for c in base]
    steps = [0, 95, 135, 175, 215, 255]
    for r in steps:
        for g in steps:
            for b in steps:
                cols.append((r << 16) | (g << 8) | b)
    for i in range(24):
        v = 8 + i * 10
        cols.append((v << 16) | (v << 8) | v)
    return ["#%06x" % c for c in cols]

PAL = palette()
SGR = re.compile(r"\x1b\[([0-9;]*)m")
OTHER = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")


def convert(text):
    out = []
    fg = bg = None
    bold = dim = italic = False
    open_span = False

    def style():
        s = []
        if fg is not None:
            s.append(f"color:{fg}")
        if bg is not None:
            s.append(f"background:{bg}")
        if bold:
            s.append("font-weight:600")
        if dim:
            s.append("opacity:.65")
        if italic:
            s.append("font-style:italic")
        return ";".join(s)

    for line in text.split("\n"):
        pos = 0
        for m in SGR.finditer(line):
            chunk = OTHER.sub("", line[pos:m.start()])
            if chunk:
                st = style()
                out.append(f'<span style="{st}">{html.escape(chunk)}</span>' if st else html.escape(chunk))
            pos = m.end()
            params = [int(p) if p else 0 for p in m.group(1).split(";")] if m.group(1) else [0]
            i = 0
            while i < len(params):
                p = params[i]
                if p == 0:
                    fg = bg = None
                    bold = dim = italic = False
                elif p == 1:
                    bold = True
                elif p == 2:
                    dim = True
                elif p == 3:
                    italic = True
                elif p == 22:
                    bold = dim = False
                elif p == 23:
                    italic = False
                elif 30 <= p <= 37:
                    fg = PAL[p - 30]
                elif 90 <= p <= 97:
                    fg = PAL[p - 90 + 8]
                elif 40 <= p <= 47:
                    bg = PAL[p - 40]
                elif 100 <= p <= 107:
                    bg = PAL[p - 100 + 8]
                elif p == 39:
                    fg = None
                elif p == 49:
                    bg = None
                elif p in (38, 48) and i + 1 < len(params):
                    if params[i + 1] == 5 and i + 2 < len(params):
                        c = PAL[params[i + 2] % 256]
                        if p == 38:
                            fg = c
                        else:
                            bg = c
                        i += 2
                    elif params[i + 1] == 2 and i + 4 < len(params):
                        c = "#%02x%02x%02x" % tuple(params[i + 2 : i + 5])
                        if p == 38:
                            fg = c
                        else:
                            bg = c
                        i += 4
                i += 1
        chunk = OTHER.sub("", line[pos:])
        if chunk:
            st = style()
            out.append(f'<span style="{st}">{html.escape(chunk)}</span>' if st else html.escape(chunk))
        out.append("\n")
    return "".join(out)


def main():
    target, out_png = sys.argv[1], sys.argv[2]
    title = sys.argv[3] if len(sys.argv) > 3 else target
    raw = subprocess.check_output(["tmux", "capture-pane", "-p", "-e", "-t", target]).decode()
    size = subprocess.check_output(["tmux", "display-message", "-p", "-t", target, "#{pane_width} #{pane_height}"]).decode().split()
    cols, rows = int(size[0]), int(size[1])
    body = convert(raw.rstrip("\n"))
    page = f"""<!doctype html><html><head><meta charset="utf-8"><title>{html.escape(title)}</title>
<style>
html,body{{margin:0;background:#101114}}
.win{{display:inline-block;background:#16171b;border-radius:12px;padding:14px 16px 16px;margin:28px;box-shadow:0 20px 60px rgba(0,0,0,.6)}}
.bar{{display:flex;gap:8px;align-items:center;margin-bottom:10px;font:12px -apple-system,Helvetica,sans-serif;color:#8b8f98}}
.dot{{width:12px;height:12px;border-radius:50%}}
pre{{margin:0;font:13.5px/1.25 "JetBrainsMono Nerd Font","JetBrains Mono","SF Mono",Menlo,monospace;color:#d4d6db;white-space:pre}}
</style></head><body><div class="win"><div class="bar"><span class="dot" style="background:#ff5f57"></span><span class="dot" style="background:#febc2e"></span><span class="dot" style="background:#28c840"></span><span style="margin-left:8px">{html.escape(title)}</span></div><pre>{body}</pre></div></body></html>"""
    with tempfile.NamedTemporaryFile("w", suffix=".html", delete=False) as f:
        f.write(page)
        path = f.name
    width = int(cols * 8.2) + 96
    height = int(rows * 17) + 110
    subprocess.check_call(
        [CHROME, "--headless=new", "--hide-scrollbars", f"--screenshot={out_png}", f"--window-size={width},{height}", "--force-device-scale-factor=2", f"file://{path}"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    os.unlink(path)
    print(out_png, f"{cols}x{rows}")


if __name__ == "__main__":
    main()
