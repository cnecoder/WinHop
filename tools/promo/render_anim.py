# -*- coding: utf-8 -*-
"""WinHop 宣传视频 —— 抽象演示动画渲染器（全局 3.0s ~ 45.5s，30fps）
四场景结构：
  01 单字母模式 · 单窗口   02 单字母模式 · 多窗口
  03 多字母模式 · 单窗口   04 多字母模式 · 多窗口
视觉语言：抽象窗口色块 + WinHop 风格覆盖层示意 + 键帽按键提示 + 底部字幕 + 顶部场景标签。
输出：JPG 帧序列到 anim_frames/
"""
import math
import os
import sys

import numpy as np
from PIL import Image, ImageDraw

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from gen_assets import (BG, FG, DIM, OFF, ACCENT, ACCENT_S, F_BOLD, F_REG, F_MONO, F_SEGOE)

W, H = 1920, 1080
FPS = 30
T0, T1 = 3.0, 47.0
LANG = os.environ.get("PROMO_LANG", "zh")          # zh | en
OUT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "anim_frames", LANG)

UI = {
    "zh": dict(
        badge="选择程序", badge_multi="多字母", setting="F2 设置     Esc 退出",
        filter="筛选", wins_one="{n} 个窗口", wins_many="{n} 个窗口", preview="窗口预览",
        explorer="文件资源管理器",
    ),
    "en": dict(
        badge="Programs", badge_multi="Multi-letter", setting="F2 Settings    Esc Close",
        filter="Filter", wins_one="{n} window", wins_many="{n} windows", preview="Window Preview",
        explorer="File Explorer",
    ),
}
U = UI[LANG]


# ================= 工具 =================
def clamp01(x):
    return max(0.0, min(1.0, x))


def smoothstep(x):
    x = clamp01(x)
    return x * x * (3 - 2 * x)


def segp(t, t0, t1):
    if t1 <= t0:
        return 1.0 if t >= t1 else 0.0
    return smoothstep((t - t0) / (t1 - t0))


def lerp(a, b, p):
    return a + (b - a) * p


def dimc(c, dim=0.0, a=1.0):
    r, g, b = c[0], c[1], c[2]
    base_a = c[3] if len(c) > 3 else 255
    k = 1.0 - 0.60 * dim
    return (int(r * k), int(g * k), int(b * k),
            int(clamp01(base_a / 255.0 * a * (1.0 - 0.30 * dim)) * 255))


def make_base():
    img = Image.new("RGBA", (W, H), BG)
    yy, xx = np.mgrid[0:H, 0:W]
    r = np.sqrt((xx - W / 2) ** 2 + (yy - H / 2) ** 2)
    al = (np.clip(1 - r / 1500, 0, 1) ** 2 * 14).astype(np.uint8)
    glow = Image.new("RGBA", (W, H), ACCENT[:3] + (0,))
    glow.putalpha(Image.fromarray(al))
    img.alpha_composite(glow)
    return img.convert("RGB")


BASE = make_base()
BASE_RGBA = BASE.convert("RGBA")

# ================= 应用 / 窗口定义 =================
APPS = {
    "chrome":   dict(bar=(58, 96, 180),  icon=(66, 133, 244),  mark="C"),
    "vscode":   dict(bar=(48, 92, 178),  icon=(56, 110, 205),  mark="V"),
    "terminal": dict(bar=(36, 74, 62),   icon=(30, 92, 72),    mark=">"),
    "explorer": dict(bar=(150, 118, 60), icon=(196, 156, 74),  mark="F"),
}
APP_TITLES = {
    "chrome": "Google Chrome",
    "vscode": "main.rs — WinHop",
    "terminal": "PowerShell — WinHop",
}

GEO = {
    "chrome":   dict(front=(360, 185, 1200, 710), back=(140, 130, 640, 432)),
    "vscode":   dict(front=(360, 185, 1200, 710), back=(560, 300, 820, 530)),
    "terminal": dict(front=(360, 185, 1200, 710), back=(1050, 450, 700, 430)),
}

# 每个窗口的前/后台关键帧 (t, state)
KEYFRAMES = {
    "vscode":   [(3.0, "front"), (10.0, "front"), (10.8, "back"), (21.0, "back"),
                 (21.8, "front"), (27.5, "front"), (28.3, "back"), (38.2, "back"),
                 (39.0, "front"), (43.6, "front"), (44.0, "back"), (46.3, "back"),
                 (46.7, "front"), (47.0, "front")],
    "chrome":   [(3.0, "back"), (10.0, "back"), (10.8, "front"), (21.0, "front"),
                 (21.8, "back"), (27.5, "back"), (28.3, "front"), (38.2, "front"),
                 (39.0, "back"), (43.6, "back"), (44.0, "front"), (46.3, "front"),
                 (46.7, "back"), (47.0, "back")],
    "terminal": [(3.0, "back"), (47.0, "back")],
}


def win_state(t, key):
    kf = KEYFRAMES[key]

    def st(name):
        x, y, w, h = GEO[key][name]
        return (x, y, w, h, 0.0 if name == "front" else 0.58)

    for i in range(len(kf) - 1):
        t0, s0 = kf[i]
        t1, s1 = kf[i + 1]
        if t0 <= t < t1:
            v0, v1 = st(s0), st(s1)
            if s0 == s1:
                return v0
            p = smoothstep((t - t0) / (t1 - t0))
            return tuple(lerp(a, b, p) for a, b in zip(v0, v1))
    return st(kf[-1][1])


# ================= 窗口内容绘制 =================
def content_lines(d, x, y, w, h, app, dim, a, seed=1):
    if a <= 0.01 or w < 20 or h < 20:
        return
    if app == "chrome":
        d.rounded_rectangle([x + w * 0.28, y + 14, x + w * 0.72, y + 48], radius=17,
                            fill=dimc((255, 255, 255, 20), dim, a))
        yy = y + 84
        for fr in [0.62, 0.78, 0.5, 0.7, 0.42, 0.66]:
            if yy > y + h - 30:
                break
            d.rounded_rectangle([x + w * 0.12, yy, x + w * (0.12 + fr * 0.76), yy + 20],
                                radius=10, fill=dimc((255, 255, 255, 24), dim, a))
            yy += 44
        s = min(w, h) * 0.11
        d.ellipse([x + w / 2 - s / 2, y + h * 0.56, x + w / 2 + s / 2, y + h * 0.56 + s],
                  fill=dimc((66, 133, 244, 110), dim, a))
    elif app == "vscode":
        side = max(8, int(w * 0.045))
        d.rectangle([x, y, x + side, y + h], fill=dimc((255, 255, 255, 15), dim, a))
        yy = y + 24
        cols = [(120, 170, 255), (140, 220, 170), (230, 200, 120), (200, 150, 220), (150, 200, 230)]
        lens = [0.30, 0.46, 0.22, 0.38, 0.52, 0.28, 0.44, 0.34, 0.5, 0.24]
        i = 0
        while yy < y + h - 34:
            c = cols[i % len(cols)]
            ln = lens[(i + seed) % len(lens)]
            d.rounded_rectangle([x + side + 22, yy, x + side + 22 + (w - side - 40) * ln,
                                 yy + max(10, int(h * 0.022))],
                                radius=7, fill=dimc(c, dim, a * 0.85))
            yy += max(26, int(h * 0.055))
            i += 1
    elif app == "terminal":
        d.text((x + 30, y + 20), ">_", font=F_MONO(32), fill=dimc((76, 222, 156), dim, a))
        yy = y + 76
        for l, fr in enumerate([0.35, 0.5, 0.28, 0.42, 0.56, 0.3, 0.46]):
            if yy > y + h - 30:
                break
            c = (76, 222, 156) if l % 2 == 0 else (200, 205, 215)
            d.rounded_rectangle([x + 30, yy, x + 30 + (w - 70) * fr, yy + 15],
                                radius=7, fill=dimc(c, dim, a * 0.8))
            yy += 38
    else:
        for r in range(3):
            yy = y + 26 + r * 88
            if yy > y + h - 60:
                break
            d.rounded_rectangle([x + 26, yy, x + w - 26, yy + 62], radius=10,
                                fill=dimc((255, 255, 255, 15), dim, a))
            d.rounded_rectangle([x + 42, yy + 15, x + 42 + w * 0.22, yy + 45], radius=8,
                                fill=dimc((196, 156, 74), dim, a * 0.75))


def draw_app_window(d, x, y, w, h, app, title, dim=0.0, a=1.0, seed=1):
    x, y, w, h = int(x), int(y), int(w), int(h)
    if a <= 0.01 or w < 40 or h < 40:
        return
    ap = APPS[app]
    d.rounded_rectangle([x, y, x + w, y + h], radius=16,
                        fill=dimc((17, 20, 26), dim, a),
                        outline=dimc((255, 255, 255, 30), dim, a), width=2)
    tb = max(40, int(h * 0.085))
    d.rounded_rectangle([x + 5, y + 5, x + w - 5, y + tb], radius=13, fill=dimc(ap["bar"], dim, a))
    cx = x + 26
    for i, c in enumerate([(255, 106, 92), (245, 191, 79), (98, 197, 84)]):
        d.ellipse([cx + i * 26, y + tb / 2 - 7, cx + i * 26 + 14, y + tb / 2 + 7],
                  fill=dimc(c, dim, a * 0.9))
    d.text((x + w / 2, y + tb / 2 + 1), title, font=F_REG(max(20, int(tb * 0.46))),
           fill=dimc(FG, dim, a * 0.92), anchor="mm")
    content_lines(d, x + 5, y + tb + 6, w - 10, h - tb - 12, app, dim, a, seed)


# ================= 覆盖层 =================
ROWS = [
    dict(key="c", multi="c",  app="chrome",   name="Google Chrome",      wins=1),
    dict(key="v", multi="vs", app="vscode",   name="Visual Studio Code", wins=3),
    dict(key="t", multi="te", app="terminal", name="Windows Terminal",   wins=2),
    dict(key="f", multi="fi", app="explorer", name=U["explorer"],        wins=1),
]
VS_WINS = [
    dict(num="1", title="main.rs — WinHop"),
    dict(num="2", title="lib.rs — WinHop"),
    dict(num="3", title="index.html"),
]
LX, LW = 340, 1240
LY0, RH, RGAP = 210, 120, 18


def match_score(r, t):
    """与 src-tauri/src/lib.rs::match_score 对齐：多字母代号 1000/800/600，名称 500/400/300"""
    mk = r["multi"].lower()
    name = r["name"].lower()
    s = 0
    if mk == t:
        s = max(s, 1000)
    elif mk.startswith(t):
        s = max(s, 800)
    elif t in mk:
        s = max(s, 600)
    if name == t:
        s = max(s, 500)
    elif name.startswith(t):
        s = max(s, 400)
    elif t in name:
        s = max(s, 300)
    return s


def match_rows(text):
    if not text:
        return [0, 1, 2, 3]
    t = text.lower()
    scored = [(i, match_score(r, t)) for i, r in enumerate(ROWS)]
    scored = [(i, s) for i, s in scored if s > 0]
    scored.sort(key=lambda x: -x[1])
    return [i for i, _ in scored]


# 筛选事件：(时间, 筛选文本) —— 空串表示重置
FILTER_EVENTS = [
    (25.25, "c"), (28.2, ""),
    (33.05, "v"), (33.80, "vs"), (39.0, ""),
]


def current_filter(t):
    txt = ""
    for (te, s) in FILTER_EVENTS:
        if te <= t:
            txt = s
    return txt


def filter_layout(t):
    """返回 4 行的 (目标行位索引 or None, 可见度)"""
    cur_text, t_cur = "", -99.0
    prev_text = ""
    for (te, s) in FILTER_EVENTS:
        if te <= t:
            prev_text = cur_text
            cur_text, t_cur = s, te
    vis_cur = match_rows(cur_text)
    vis_prev = match_rows(prev_text)
    p = segp(t, t_cur, t_cur + 0.38) if t_cur > 0 else 1.0
    out = []
    for i in range(4):
        if i in vis_cur:
            target = vis_cur.index(i)
            start = vis_prev.index(i) if i in vis_prev else target
            out.append((lerp(start, target, p), 1.0))
        else:
            a = (1 - segp(t, t_cur, t_cur + 0.22)) if i in vis_prev else 0.0
            out.append((None, max(0.0, a)))
    return out


def draw_overlay_panel(d, off, a, badge=None):
    if badge is None:
        badge = U["badge"]
    d.rectangle([0, 0, W, H], fill=(11, 12, 15, int(242 * a)))
    y = off
    d.text((LX, y + 118), "WinHop", font=F_BOLD(46), fill=dimc(FG, 0, a), anchor="lm")
    d.rounded_rectangle([LX + 225, y + 96, LX + 399, y + 140], radius=22,
                        fill=(53, 208, 122, int(24 * a)),
                        outline=(53, 208, 122, int(88 * a)), width=2)
    d.text((LX + 312, y + 119), badge, font=F_REG(26), fill=dimc(ACCENT_S, 0, a), anchor="mm")
    d.text((LX + LW, y + 118), U["setting"], font=F_REG(26), fill=dimc(OFF, 0, a), anchor="rm")


def draw_filter_bar(d, off, a, text):
    if a <= 0.01:
        return
    y = off + 152
    d.text((LX + 6, y + 23), U["filter"], font=F_REG(26), fill=dimc(OFF, 0, a), anchor="lm")
    bx, bw, bh = LX + 84, 320, 46
    d.rounded_rectangle([bx, y, bx + bw, y + bh], radius=12,
                        fill=(255, 255, 255, int(10 * a)),
                        outline=(53, 208, 122, int(140 * a)), width=2)
    if text:
        f = F_MONO(30)
        d.text((bx + 22, y + bh / 2 + 1), text, font=f, fill=dimc(FG, 0, a), anchor="lm")
        tw = d.textlength(text, font=f)
        d.rectangle([bx + 26 + tw, y + 13, bx + 30 + tw, y + bh - 13],
                    fill=(53, 208, 122, int(210 * a)))


def draw_row(d, i, y, sel, a, wins_pulse=0.0, multi_mode=False):
    x = LX
    fill = (int(lerp(255, ACCENT[0], sel * 0.20)), int(lerp(255, ACCENT[1], sel * 0.20)),
            int(lerp(255, ACCENT[2], sel * 0.20)), int(9 * a + 16 * sel * a))
    if sel > 0.02:
        d.rounded_rectangle([x, y, x + LW, y + RH], radius=14, fill=fill,
                            outline=(53, 208, 122, int(205 * sel * a)), width=3)
    else:
        d.rounded_rectangle([x, y, x + LW, y + RH], radius=14, fill=fill,
                            outline=(255, 255, 255, int(17 * a)), width=2)
    row = ROWS[i]
    # 多字母模式显示 multi_key（如 c / vs），单字母模式显示 key（如 c / v）
    label = row["multi"].upper() if multi_mode else row["key"].upper()
    kfs = 38 if len(label) <= 1 else 29
    kx, ky, ks = x + 26, y + (RH - 64) // 2, 64
    if sel > 0.5:
        d.rounded_rectangle([kx, ky, kx + ks, ky + ks], radius=13, fill=dimc(ACCENT, 0, a))
        d.text((kx + ks / 2, ky + ks / 2 + 2), label, font=F_MONO(kfs),
               fill=dimc((4, 36, 15), 0, a), anchor="mm")
    else:
        d.rounded_rectangle([kx, ky, kx + ks, ky + ks], radius=13, fill=(255, 255, 255, int(16 * a)),
                            outline=(255, 255, 255, int(40 * a)), width=2)
        d.text((kx + ks / 2, ky + ks / 2 + 2), label, font=F_MONO(kfs),
               fill=dimc(FG, 0, a * 0.95), anchor="mm")
    ix, isz = kx + ks + 26, 56
    iy = y + (RH - isz) // 2
    d.rounded_rectangle([ix, iy, ix + isz, iy + isz], radius=15, fill=dimc(APPS[row["app"]]["icon"], 0, a))
    d.text((ix + isz / 2, iy + isz / 2 + 2), APPS[row["app"]]["mark"], font=F_MONO(30),
           fill=(255, 255, 255, int(255 * a)), anchor="mm")
    d.text((ix + isz + 28, y + RH / 2 + 1), row["name"], font=F_REG(40), fill=dimc(FG, 0, a), anchor="lm")
    n = row["wins"]
    wl = (U["wins_one"] if n == 1 else U["wins_many"]).format(n=n)
    wf = F_REG(28)
    if wins_pulse > 0.03:
        tw = d.textlength(wl, font=wf)
        d.rounded_rectangle([x + LW - 30 - tw - 18, y + RH / 2 - 24, x + LW - 12, y + RH / 2 + 24],
                            radius=24, fill=(53, 208, 122, int(26 * wins_pulse * a)),
                            outline=(53, 208, 122, int(160 * wins_pulse * a)), width=2)
        d.text((x + LW - 30, y + RH / 2 + 1), wl, font=wf, fill=dimc(ACCENT_S, 0, a), anchor="rm")
    else:
        d.text((x + LW - 30, y + RH / 2 + 1), wl, font=wf, fill=dimc(OFF, 0, a), anchor="rm")


def draw_window_layer(d, off, a, sel=1, preview_idx=1):
    y0 = LY0 + off
    x = LX
    lw = 660
    for i, wnd in enumerate(VS_WINS):
        y = y0 + i * 164
        s = 1.0 if i == sel else 0.0
        d.rounded_rectangle([x, y, x + lw, y + 150], radius=14,
                            fill=(28, 60, 40, int(120 * a)) if s else (255, 255, 255, int(8 * a)),
                            outline=(53, 208, 122, int(215 * a)) if s else (255, 255, 255, int(16 * a)),
                            width=3 if s else 2)
        kx, ky, ks = x + 24, y + 49, 52
        if s:
            d.rounded_rectangle([kx, ky, kx + ks, ky + ks], radius=12, fill=dimc(ACCENT, 0, a))
            d.text((kx + ks / 2, ky + ks / 2 + 2), wnd["num"], font=F_MONO(32),
                   fill=dimc((4, 36, 15), 0, a), anchor="mm")
        else:
            d.rounded_rectangle([kx, ky, kx + ks, ky + ks], radius=12, fill=(255, 255, 255, int(14 * a)),
                                outline=(255, 255, 255, int(36 * a)), width=2)
            d.text((kx + ks / 2, ky + ks / 2 + 2), wnd["num"], font=F_MONO(32),
                   fill=dimc(FG, 0, a * 0.9), anchor="mm")
        tx = kx + ks + 22
        d.rounded_rectangle([tx, y + 20, tx + 190, y + 130], radius=10,
                            fill=dimc((17, 20, 26), 0, a),
                            outline=(255, 255, 255, int(30 * a)), width=2)
        content_lines(d, tx + 6, y + 26, 178, 98, "vscode", 0, a * 0.95, seed=i)
        d.text((tx + 216, y + 76), wnd["title"], font=F_REG(30), fill=dimc(FG, 0, a), anchor="lm")
    px, pw = 1060, 560
    d.rounded_rectangle([px, y0, px + pw, y0 + 690], radius=18,
                        fill=dimc((15, 17, 22), 0, a),
                        outline=(255, 255, 255, int(30 * a)), width=2)
    d.text((px + pw / 2, y0 + 42), U["preview"], font=F_REG(26), fill=dimc(OFF, 0, a), anchor="mm")
    pv_w, pv_h = pw - 80, 480
    pv_x, pv_y = px + 40, y0 + 92
    d.rounded_rectangle([pv_x, pv_y, pv_x + pv_w, pv_y + pv_h], radius=12,
                        fill=dimc((17, 20, 26), 0, a),
                        outline=(255, 255, 255, int(34 * a)), width=2)
    tbh = 46
    d.rounded_rectangle([pv_x + 4, pv_y + 4, pv_x + pv_w - 4, pv_y + tbh], radius=10,
                        fill=dimc(APPS["vscode"]["bar"], 0, a))
    d.text((pv_x + pv_w / 2, pv_y + tbh / 2 + 2), VS_WINS[preview_idx]["title"],
           font=F_REG(24), fill=(255, 255, 255, int(235 * a)), anchor="mm")
    content_lines(d, pv_x + 6, pv_y + tbh + 8, pv_w - 12, pv_h - tbh - 14, "vscode", 0, a,
                  seed=preview_idx * 3)


# ================= 场景标签 =================
if LANG == "zh":
    SCENE_LABELS = [
        ("01", "单字母模式 · 单窗口", 7.95, 14.45),
        ("02", "单字母模式 · 多窗口", 14.60, 22.95),
        ("03", "多字母模式 · 单窗口", 23.05, 30.75),
        ("04", "多字母模式 · 多窗口", 31.05, 40.35),
    ]
else:
    SCENE_LABELS = [
        ("01", "Single-Letter · Single Window", 7.95, 14.45),
        ("02", "Single-Letter · Multi-Window", 14.60, 22.95),
        ("03", "Multi-Letter · Single Window", 23.05, 30.75),
        ("04", "Multi-Letter · Multi-Window", 31.05, 40.35),
    ]


def draw_scene_labels(d, t):
    for num, text, s0, s1 in SCENE_LABELS:
        if s0 <= t <= s1:
            a = segp(t, s0, s0 + 0.35) * (1 - segp(t, s1 - 0.3, s1))
            if a <= 0.01:
                continue
            f1 = F_MONO(24)
            f2 = F_REG(27) if LANG == "zh" else F_SEGOE(23)
            w1 = d.textlength(num, font=f1)
            w2 = d.textlength(text, font=f2)
            g1, pad = 16, 30
            total = pad * 2 + w1 + g1 + w2
            x = W / 2 - total / 2
            d.rounded_rectangle([x, 20, x + total, 74], radius=27,
                                fill=(53, 208, 122, int(16 * a)),
                                outline=(53, 208, 122, int(85 * a)), width=2)
            cx = x + pad
            d.text((cx, 48), num, font=f1, fill=dimc(ACCENT_S, 0, a), anchor="lm")
            cx += w1 + g1
            d.text((cx, 48), text, font=f2, fill=dimc(FG, 0, a), anchor="lm")


# ================= 键帽提示 =================
KEYHINTS = [
    (3.7, 5.0,    ["Ctrl", "Space"], [(4.45, 4.75)]),
    (8.2, 9.0,    ["C"], [(8.55, 8.85)]),
    (14.7, 15.9,  ["Ctrl", "Space"], [(15.15, 15.45)]),
    (16.6, 17.9,  ["V"], [(17.40, 17.70)]),
    (19.6, 20.5,  ["2"], [(19.90, 20.20)]),
    (23.1, 24.2,  ["Ctrl", "Space"], [(23.55, 23.85)]),
    (24.9, 25.55, ["C"], [(25.20, 25.50)]),
    (26.35, 27.35, ["Enter"], [(27.00, 27.30)]),
    (31.1, 32.1,  ["Ctrl", "Space"], [(31.55, 31.85)]),
    (32.7, 33.35, ["V"], [(33.00, 33.30)]),
    (33.45, 34.1, ["S"], [(33.75, 34.05)]),
    (34.4, 35.15, ["Enter"], [(34.70, 35.00)]),
    (36.7, 37.35, ["3"], [(36.95, 37.25)]),
    # 空格快速跳转：先 Ctrl+Space 呼出，再按 Space 跳到上一个窗口（两次来回）
    (41.1, 42.85, ["Ctrl", "Space"], [(41.50, 41.80)]),
    (42.55, 43.45, ["Space"], [(43.00, 43.30)]),
    (44.2, 45.95, ["Ctrl", "Space"], [(44.60, 44.90)]),
    (45.65, 46.45, ["Space"], [(46.00, 46.30)]),
]


def keyhint_at(t):
    for (s0, s1, keys, presses) in KEYHINTS:
        if s0 <= t < s1:
            a = segp(t, s0, s0 + 0.45) * (1 - segp(t, s1 - 0.4, s1))
            pr = 0.0
            for (p0, p1) in presses:
                if t >= p0:
                    pr = max(pr, segp(t, p0, p0 + 0.12) * (1 - segp(t, p1 - 0.12, p1)))
            return keys, min(1.0, pr), a
    return None


def draw_keycap(d, x, y, label, font, pressed=0.0, a=1.0):
    tw = d.textlength(label, font=font)
    w = int(tw + 68)
    h = int(font.size + 40)
    yy = y + 5 * pressed
    if pressed > 0.02:
        for k, al in [(12, 36), (7, 64), (3, 100)]:
            d.rounded_rectangle([x - k, yy - k, x + w + k, yy + h + k], radius=16 + k,
                                outline=(53, 208, 122, int(al * pressed * a)), width=3)
    border = (53, 208, 122, int(235 * a)) if pressed > 0.3 else (255, 255, 255, int(52 * a))
    d.rounded_rectangle([x, yy, x + w, yy + h], radius=16,
                        fill=(int(lerp(24, 22, pressed)), int(lerp(28, 52, pressed)),
                              int(lerp(36, 34, pressed)), int(250 * a)),
                        outline=border, width=3)
    tc = tuple(int(lerp(c, s, pressed)) for c, s in zip(FG[:3], ACCENT_S[:3]))
    d.text((x + w / 2, yy + h / 2 + 1), label, font=font, fill=tc + (int(255 * a),), anchor="mm")
    return w, h


def draw_keyhint(img, keys, pressed, a):
    if a <= 0.01:
        return
    d = ImageDraw.Draw(img)
    f = F_MONO(40)
    widths = [int(d.textlength(k, font=f) + 68) for k in keys]
    plus_gap = 44 if len(keys) > 1 else 0
    total = sum(widths) + plus_gap * (len(keys) - 1)
    x = W / 2 - total / 2
    y = 756
    for i, k in enumerate(keys):
        draw_keycap(d, x, y, k, f, pressed=pressed, a=a)
        x += widths[i]
        if i < len(keys) - 1:
            d.text((x + plus_gap / 2, y + 47), "+", font=F_REG(36), fill=dimc(OFF, 0, a), anchor="mm")
            x += plus_gap


# ================= 字幕（单语：按 LANG 切换）=================
if LANG == "zh":
    CAP_FS = 46
    CAPTIONS = {
        1: [("Ctrl+Space", F_MONO(46), ACCENT_S), (" 呼出全部程序", F_REG(46), FG)],
        2: [("按", F_REG(46), FG), ("字母", F_BOLD(46), ACCENT_S), ("，选中程序", F_REG(46), FG)],
        3: [("单窗口程序：", F_REG(46), FG), ("一次按键，直接到达", F_BOLD(46), ACCENT_S)],
        4: [("多窗口程序，进入", F_REG(46), FG), ("窗口层", F_BOLD(46), ACCENT_S)],
        5: [("按", F_REG(46), FG), ("数字", F_BOLD(46), ACCENT_S), ("，选择第几个窗口", F_REG(46), FG)],
        6: [("多字母模式", F_BOLD(46), ACCENT_S), ("：按代号实时筛选", F_REG(46), FG)],
        7: [("列表按", F_REG(46), FG), ("代号 / 名称", F_BOLD(46), ACCENT_S), ("实时筛选", F_REG(46), FG)],
        8: [("Enter", F_MONO(46), ACCENT_S), (" 确认，单窗口直接切换", F_REG(46), FG)],
        9: [("Enter", F_MONO(46), ACCENT_S), (" 确认最佳匹配", F_REG(46), FG)],
        10: [("多窗口程序，进入", F_REG(46), FG), ("窗口层", F_BOLD(46), ACCENT_S)],
        11: [("按", F_REG(46), FG), ("数字", F_BOLD(46), ACCENT_S), ("，选择第几个窗口", F_REG(46), FG)],
        12: [("多窗口：先进", F_REG(46), FG), ("窗口层", F_BOLD(46), ACCENT_S), ("，再选窗口", F_REG(46), FG)],
        13: [("空格 = ", F_REG(46), FG), ("最近两个窗口", F_BOLD(46), ACCENT_S), (" 来回跳", F_REG(46), FG)],
    }
else:
    CAP_FS = 42
    CAPTIONS = {
        1: [("Ctrl+Space", F_MONO(42), ACCENT_S), (" to summon every app", F_SEGOE(42), FG)],
        2: [("Press a letter", F_SEGOE(42), ACCENT_S), (" to pick the app", F_SEGOE(42), FG)],
        3: [("Single window", F_SEGOE(42), ACCENT_S), (" — one key, straight there", F_SEGOE(42), FG)],
        4: [("Multi-window app", F_SEGOE(42), ACCENT_S), (" — opens the window list", F_SEGOE(42), FG)],
        5: [("Press a number", F_SEGOE(42), ACCENT_S), (" to pick the window", F_SEGOE(42), FG)],
        6: [("Multi-letter mode", F_SEGOE(42), ACCENT_S), (" — filter by code", F_SEGOE(42), FG)],
        7: [("The list filters", F_SEGOE(42), ACCENT_S), (" live by key or name", F_SEGOE(42), FG)],
        8: [("Enter", F_MONO(42), ACCENT_S), (" confirms — switch instantly", F_SEGOE(42), FG)],
        9: [("Enter", F_MONO(42), ACCENT_S), (" confirms the best match", F_SEGOE(42), FG)],
        10: [("Multi-window app", F_SEGOE(42), ACCENT_S), (" — opens the window list", F_SEGOE(42), FG)],
        11: [("Press a number", F_SEGOE(42), ACCENT_S), (" to pick the window", F_SEGOE(42), FG)],
        12: [("Multi-window", F_SEGOE(42), ACCENT_S), (" — window list first, then pick", F_SEGOE(42), FG)],
        13: [("Space", F_MONO(42), ACCENT_S), (" — toggle between the two recent windows", F_SEGOE(42), FG)],
    }
CAP_TIMES = {
    1: (5.6, 7.9),
    2: (9.0, 10.1), 3: (11.0, 14.3),
    4: (18.5, 19.7), 5: (20.4, 21.2),
    6: (24.6, 25.8), 7: (25.9, 27.0), 8: (28.6, 30.4),
    9: (34.2, 35.1), 10: (35.7, 36.7), 11: (37.5, 38.2), 12: (39.4, 40.3),
    13: (40.7, 46.9),
}
_cap_cache = {}


def cap_png(idx):
    if idx not in _cap_cache:
        segs = CAPTIONS[idx]
        strip = Image.new("RGBA", (W, 170), (0, 0, 0, 0))
        d = ImageDraw.Draw(strip)
        widths = [d.textlength(t, font=f) for t, f, c in segs]
        total = sum(widths)
        pad_x, pad_y = 66, 26
        bw, bh = total + pad_x * 2, CAP_FS + pad_y * 2 + 12
        bx, by = W / 2 - bw / 2, 85 - bh / 2
        d.rounded_rectangle([bx, by, bx + bw, by + bh], radius=30,
                            fill=(13, 15, 20, 236), outline=(255, 255, 255, 34), width=2)
        x = W / 2 - total / 2
        for (t, f, c), w in zip(segs, widths):
            d.text((x, 86), t, font=f, fill=c, anchor="lm")
            x += w
        _cap_cache[idx] = strip
    return _cap_cache[idx]


def draw_captions(img, t):
    for idx, (t0, t1) in CAP_TIMES.items():
        if t0 <= t <= t1 + 0.2:
            a = segp(t, t0, t0 + 0.18) * (1 - segp(t, t1, t1 + 0.2))
            if a > 0.01:
                png = cap_png(idx)
                if a < 0.995:
                    png = png.copy()
                    png.putalpha(png.getchannel("A").point(lambda v: int(v * a)))
                img.alpha_composite(png, (0, 872))


# ================= 场景调度 =================
def overlay_alpha(t):
    # S1 开场呼出
    if 4.75 <= t < 5.30:
        return segp(t, 4.75, 5.30)
    if 5.30 <= t < 10.0:
        return 1.0
    if 10.0 <= t < 10.6:
        return 1 - segp(t, 10.0, 10.6)
    # 场景 02
    if 15.45 <= t < 15.95:
        return segp(t, 15.45, 15.95)
    if 15.95 <= t < 21.2:
        return 1.0
    if 21.2 <= t < 21.8:
        return 1 - segp(t, 21.2, 21.8)
    # 场景 03
    if 23.85 <= t < 24.35:
        return segp(t, 23.85, 24.35)
    if 24.35 <= t < 27.5:
        return 1.0
    if 27.5 <= t < 28.1:
        return 1 - segp(t, 27.5, 28.1)
    # 场景 04
    if 31.85 <= t < 32.35:
        return segp(t, 31.85, 32.35)
    if 32.35 <= t < 38.2:
        return 1.0
    if 38.2 <= t < 38.8:
        return 1 - segp(t, 38.2, 38.8)
    # 空格快速跳转：第一次呼出 → Space
    if 41.75 <= t < 42.10:
        return segp(t, 41.75, 42.10)
    if 42.10 <= t < 43.30:
        return 1.0
    if 43.30 <= t < 43.65:
        return 1 - segp(t, 43.30, 43.65)
    # 空格快速跳转：第二次呼出 → Space
    if 44.85 <= t < 45.20:
        return segp(t, 44.85, 45.20)
    if 45.20 <= t < 46.30:
        return 1.0
    if 46.30 <= t < 46.65:
        return 1 - segp(t, 46.30, 46.65)
    return 0.0


def render_frame(t):
    # 全部元素画到透明图层，最后与底图做 alpha 合成
    ov = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(ov)

    # ---- 1) 桌面窗口 ----
    enter = segp(t, 3.0, 3.6)
    order = sorted(["chrome", "vscode", "terminal"], key=lambda k: -win_state(t, k)[4])
    for key in order:
        x, y, w, h, dim = win_state(t, key)
        title = APP_TITLES[key]
        if key == "vscode":
            if t >= 39.0:
                title = "index.html — WinHop"
            elif t >= 21.3:
                title = "lib.rs — WinHop"
        seed = {"chrome": 1, "vscode": 0, "terminal": 2}[key]
        draw_app_window(d, x, y + (1 - enter) * 26, w, h, key, title, dim=dim, a=enter, seed=seed)

    # ---- 2) 覆盖层 ----
    ov_a = overlay_alpha(t)
    if ov_a > 0.01:
        off = (1 - ov_a) * 34
        d.rectangle([0, 0, W, H], fill=(11, 12, 15, int(205 * ov_a)))
        multi = (23.8 <= t < 28.1) or (31.8 <= t < 38.8)
        draw_overlay_panel(d, off, ov_a, badge=(U["badge_multi"] if multi else U["badge"]))
        if multi:
            ftext = current_filter(t)
            if t < 28:
                fa = segp(t, 23.9, 24.45) * ov_a * (1 - segp(t, 27.6, 28.0))
            else:
                fa = segp(t, 31.9, 32.45) * ov_a * (1 - segp(t, 38.4, 38.7))
            draw_filter_bar(d, off, fa, ftext)

        # 列表 / 窗口层切换
        list_a, wl_a = ov_a, 0.0
        if 17.9 <= t < 18.5:
            p = segp(t, 17.9, 18.4)
            list_a, wl_a = ov_a * (1 - p), ov_a * p
        elif 18.5 <= t < 21.2:
            list_a, wl_a = 0.0, ov_a
        elif 35.1 <= t < 35.7:
            p = segp(t, 35.1, 35.6)
            list_a, wl_a = ov_a * (1 - p), ov_a * p
        elif 35.7 <= t < 38.2:
            list_a, wl_a = 0.0, ov_a

        if list_a > 0.01:
            if t < 6.3:
                # S1 呼出：行级联入场
                for i in range(4):
                    p = segp(t, 5.30 + i * 0.13, 5.70 + i * 0.13)
                    if p > 0.01:
                        draw_row(d, i, LY0 + off + i * (RH + RGAP) + (1 - p) * 24, 0.0, p * list_a)
            else:
                sel_c = max(
                    segp(t, 8.60, 9.00) if 8.5 <= t < 10.2 else 0.0,
                    segp(t, 27.05, 27.40) if 27.0 <= t < 28.0 else 0.0,
                )
                sel_v = max(
                    segp(t, 17.45, 17.85) if 17.4 <= t < 18.6 else 0.0,
                    segp(t, 34.75, 35.10) if 34.7 <= t < 35.9 else 0.0,
                )
                wpulse = (0.5 + 0.5 * math.sin((t - 6.4) * 2 * math.pi / 0.9)) if 6.4 <= t <= 7.6 else 0.0
                rows_state = filter_layout(t)
                for i in range(4):
                    y_idx, ra = rows_state[i]
                    if y_idx is None or ra <= 0.01:
                        continue
                    sel = sel_c if i == 0 else (sel_v if i == 1 else 0.0)
                    draw_row(d, i, LY0 + off + y_idx * (RH + RGAP), sel, ra * list_a,
                             wins_pulse=(wpulse if i == 1 else 0.0), multi_mode=multi)

        if wl_a > 0.01:
            if t < 23.0:   # 场景 02：按 2 → 第 2 个窗口
                mix = segp(t, 20.20, 20.50) if t >= 20.2 else 0.0
                idx = 0 if mix < 0.5 else 1
            else:          # 场景 04：按 3 → 第 3 个窗口
                mix = segp(t, 37.25, 37.55) if t >= 37.25 else 0.0
                idx = 0 if mix < 0.5 else 2
            draw_window_layer(d, off, wl_a, sel=idx, preview_idx=idx)

    # ---- 3) 场景标签 ----
    draw_scene_labels(d, t)

    # ---- 4) 键帽提示 ----
    kh = keyhint_at(t)
    if kh and kh[2] > 0.01:
        draw_keyhint(ov, kh[0], kh[1], kh[2])

    # ---- 5) 字幕 ----
    draw_captions(ov, t)

    return Image.alpha_composite(BASE_RGBA, ov).convert("RGB")


if __name__ == "__main__":
    args = sys.argv[1:]
    os.makedirs(OUT_DIR, exist_ok=True)
    if args and args[0] == "check":
        for s in args[1:]:
            img = render_frame(float(s))
            img.save(os.path.join(OUT_DIR, f"chk_{s}.png"))
            print("saved", s)
    else:
        n = int((T1 - T0) * FPS)
        import time
        t_start = time.time()
        for i in range(n):
            img = render_frame(T0 + i / FPS)
            img.save(os.path.join(OUT_DIR, f"f_{i:05d}.jpg"), quality=92)
            if i % 150 == 0:
                print(f"frame {i}/{n}  elapsed={time.time()-t_start:.0f}s", flush=True)
        print("ALL FRAMES DONE", n)
