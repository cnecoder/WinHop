# -*- coding: utf-8 -*-
"""WinHop 宣传视频静态素材生成器（PIL）
生成：开场卡 / 结尾卡 / 4 张特性卡 / 6 条字幕条
配色与排版对齐 src/styles.css 设计令牌。
绘制策略：所有元素先画在透明 overlay 上做 alpha 合成，再落到不透明底，避免
PIL 半透明色被 convert("RGB") 打平为实心的问题。
"""
from PIL import Image, ImageDraw, ImageFont, ImageFilter
import numpy as np
import os

HERE = os.path.dirname(os.path.abspath(__file__))
LANG = os.environ.get("PROMO_LANG", "zh")          # zh | en
OUT = os.path.join(HERE, "assets", LANG)
os.makedirs(OUT, exist_ok=True)

TXT = {
    "zh": dict(
        open_sub="Windows 窗口快速切换器",
        open_kbdesc="两次按键 · 直达任意窗口",
        end_sub="免费 · 开源 · Windows 窗口快速切换器",
        end_hint="在 GitHub 搜索 WinHop · 欢迎 Star 支持",
        feat_header="为什么选择 WinHop",
        features=[
            ("01", "纯键盘操作", "呼出、选择、切换，全程无需鼠标"),
            ("02", "两层模型", "程序层选应用，窗口层选窗口"),
            ("03", "轻量常驻", "托盘运行 · 单实例 · 升级不丢配置"),
            ("04", "开源免费", "代码公开，永久免费，欢迎 Star"),
        ],
    ),
    "en": dict(
        open_sub="Windows Window Quick Switcher",
        open_kbdesc="Two keystrokes · Any window",
        end_sub="Free & Open Source · Windows Window Quick Switcher",
        end_hint="Search WinHop on GitHub · Star to support",
        feat_header="Why WinHop",
        features=[
            ("01", "Keyboard Only", "Summon, pick, switch — no mouse needed"),
            ("02", "Two-Layer Model", "Pick an app, then pick its window"),
            ("03", "Lightweight", "Tray-resident · Config survives updates"),
            ("04", "Free & Open Source", "Open source, free forever, stars welcome"),
        ],
    ),
}
T = TXT[LANG]

W, H = 1920, 1080

# ---- 设计令牌（对齐 src/styles.css）----
BG       = (11, 12, 15, 255)     # #0b0c0f
FG       = (232, 235, 241, 255)  # #e8ebf1
DIM      = (154, 163, 181, 255)  # #9aa3b5
OFF      = (91, 100, 117, 255)   # #5b6475
ACCENT   = (53, 208, 122, 255)   # #35d07a
ACCENT_S = (72, 224, 140, 255)   # #48e08c

FONTS = "C:/Windows/Fonts/"
def F_BOLD(s): return ImageFont.truetype(FONTS + "msyhbd.ttc", s)
def F_REG(s):  return ImageFont.truetype(FONTS + "msyh.ttc", s)
def F_MONO(s): return ImageFont.truetype(FONTS + "consolab.ttf", s)
def F_SEGOE(s):
    try:
        return ImageFont.truetype(FONTS + "segoeui.ttf", s)
    except Exception:
        return F_REG(s)
def F_SEGOE_B(s):
    try:
        return ImageFont.truetype(FONTS + "segoeuib.ttf", s)
    except Exception:
        return F_SEGOE(s)


def make_bg(w=W, h=H, glow_center=(960, 470), glow_radius=820, glow_alpha=20):
    """深色底 + accent 径向微光（不透明底）"""
    img = Image.new("RGBA", (w, h), BG)
    yy, xx = np.mgrid[0:h, 0:w]
    r = np.sqrt((xx - glow_center[0]) ** 2 + (yy - glow_center[1]) ** 2)
    a = np.clip(1 - r / glow_radius, 0, 1) ** 2
    a = (a * glow_alpha).astype(np.uint8)
    glow = Image.new("RGBA", (w, h), ACCENT[:3] + (0,))
    glow.putalpha(Image.fromarray(a))
    img.alpha_composite(glow)
    return img


def paste_icon(canvas, size, center, glow=True):
    """把 icon.png 贴到 canvas（RGBA）上，带绿色光晕"""
    icon = Image.open(os.path.join(HERE, "..", "..", "src-tauri", "icons", "icon.png")).convert("RGBA")
    icon = icon.resize((size, size), Image.LANCZOS)
    x, y = center[0] - size // 2, center[1] - size // 2
    if glow:
        g = Image.new("RGBA", (size + 160, size + 160), (0, 0, 0, 0))
        gd = ImageDraw.Draw(g)
        gd.rounded_rectangle([80, 80, 80 + size, 80 + size], radius=int(size * 0.22),
                             fill=ACCENT[:3] + (58,))
        g = g.filter(ImageFilter.GaussianBlur(46))
        canvas.alpha_composite(g, (x - 80, y - 80))
    canvas.alpha_composite(icon, (x, y))


def draw_keycap(d, x, y, label, font, color=FG, pad_x=26, pad_y=15, radius=14):
    tw = d.textlength(label, font=font)
    w = tw + pad_x * 2
    h = font.size + pad_y * 2
    d.rounded_rectangle([x, y, x + w, y + h], radius=radius,
                        fill=(26, 30, 38, 250), outline=(255, 255, 255, 40), width=2)
    d.text((x + w / 2, y + h / 2 + 1), label, font=font, fill=color, anchor="mm")
    return w, h


def draw_rich_center(d, center, segments, gap=0):
    """segments: [(text, font, color)] 水平居中绘制，anchor lm"""
    widths = [d.textlength(t, font=f) for t, f, c in segments]
    total = sum(widths) + gap * max(0, len(segments) - 1)
    x = center[0] - total / 2
    for (t, f, c), w in zip(segments, widths):
        d.text((x, center[1]), t, font=f, fill=c, anchor="lm")
        x += w + gap


# ============ 1. 开场卡 ============
def gen_open():
    img = make_bg()
    ov = Image.new("RGBA", img.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(ov)
    paste_icon(ov, 216, (960, 368))

    d.text((960, 566), "WinHop", font=F_BOLD(130), fill=FG, anchor="mm")
    sub_font = F_REG(43) if LANG == "zh" else F_SEGOE(38)
    d.text((960, 700), T["open_sub"], font=sub_font, fill=DIM, anchor="mm")

    # 键帽组：[Ctrl] + [Space] + 说明文字
    fk = F_MONO(33)
    w1 = d.textlength("Ctrl", font=fk) + 52
    w2 = d.textlength("Space", font=fk) + 52
    h1 = fk.size + 30
    fdesc = F_REG(33) if LANG == "zh" else F_SEGOE(30)
    desc = T["open_kbdesc"]
    wd = d.textlength(desc, font=fdesc)
    plus_w = 46
    total = w1 + plus_w + w2 + 34 + wd
    x = 960 - total / 2
    y = 846
    draw_keycap(d, x, y, "Ctrl", fk)
    x += w1
    d.text((x + plus_w / 2, y + h1 / 2), "+", font=F_REG(34), fill=OFF, anchor="mm")
    x += plus_w
    draw_keycap(d, x, y, "Space", fk, color=ACCENT_S)
    x += w2 + 34
    d.text((x, y + h1 / 2), desc, font=fdesc, fill=OFF, anchor="lm")

    img.alpha_composite(ov)
    img.convert("RGB").save(os.path.join(OUT, "open_card.png"))
    print("open_card.png")


# ============ 2. 结尾卡 ============
def gen_end():
    img = make_bg(glow_center=(960, 500))
    ov = Image.new("RGBA", img.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(ov)
    paste_icon(ov, 178, (960, 360))

    d.text((960, 546), "WinHop", font=F_BOLD(122), fill=FG, anchor="mm")
    sub_font = F_REG(41) if LANG == "zh" else F_SEGOE(33)
    d.text((960, 682), T["end_sub"], font=sub_font, fill=DIM, anchor="mm")

    # GitHub 框
    f_url = F_MONO(45)
    url = "github.com/cnecoder/WinHop"
    tw = d.textlength(url, font=f_url)
    pad_x, pad_y = 52, 24
    bw, bh = tw + pad_x * 2, f_url.size + pad_y * 2
    bx, by = 960 - bw / 2, 774
    d.rounded_rectangle([bx, by, bx + bw, by + bh], radius=20,
                        fill=(53, 208, 122, 14), outline=ACCENT[:3] + (110,), width=2)
    d.text((960, by + bh / 2 + 1), url, font=f_url, fill=ACCENT_S, anchor="mm")

    hint_font = F_REG(30) if LANG == "zh" else F_SEGOE(26)
    d.text((960, 940), T["end_hint"], font=hint_font, fill=OFF, anchor="mm")

    img.alpha_composite(ov)
    img.convert("RGB").save(os.path.join(OUT, "end_card.png"))
    print("end_card.png")


# ============ 3. 特性卡 ============
def gen_feats():
    for i, (num, title, desc) in enumerate(T["features"], 1):
        img = make_bg(glow_center=(960, 480), glow_alpha=16)
        ov = Image.new("RGBA", img.size, (0, 0, 0, 0))
        d = ImageDraw.Draw(ov)

        d.text((960, 300), T["feat_header"], font=(F_REG(32) if LANG == "zh" else F_SEGOE(28)),
               fill=OFF, anchor="mm")
        d.text((960, 434), num, font=F_MONO(92), fill=ACCENT, anchor="mm")
        d.rounded_rectangle([960 - 40, 508, 960 + 40, 513], radius=3, fill=ACCENT[:3] + (150,))
        d.text((960, 584), title, font=(F_BOLD(76) if LANG == "zh" else F_SEGOE_B(62)),
               fill=FG, anchor="mm")
        d.text((960, 712), desc, font=(F_REG(38) if LANG == "zh" else F_SEGOE(31)),
               fill=DIM, anchor="mm")

        img.alpha_composite(ov)
        img.convert("RGB").save(os.path.join(OUT, f"feat{i}.png"))
        print(f"feat{i}.png")


# ============ 4. 字幕条（保持 RGBA 透明输出）============
SUBS = [
    [("Ctrl+Space", F_MONO(46), ACCENT_S), (" 呼出全屏选择页", F_REG(46), FG)],
    [("按 ", F_REG(46), FG), ("字母", F_BOLD(46), ACCENT_S), (" 选程序 · 按 ", F_REG(46), FG),
     ("数字", F_BOLD(46), ACCENT_S), (" 选窗口", F_REG(46), FG)],
    [("切换瞬时完成，", F_REG(46), FG), ("手不离键盘", F_BOLD(46), ACCENT_S)],
    [("空格 = ", F_REG(46), FG), ("最近两个窗口", F_BOLD(46), ACCENT_S), (" 来回跳", F_REG(46), FG)],
    [("窗口层：", F_REG(46), FG), ("缩略图 + 大预览", F_BOLD(46), ACCENT_S), (" 再选中", F_REG(46), FG)],
    [("Esc ", F_MONO(46), ACCENT_S), ("逐层回退 · 全键盘操作", F_REG(46), FG)],
]

def gen_subs():
    SW, SH = 1920, 170
    for i, segs in enumerate(SUBS, 1):
        strip = Image.new("RGBA", (SW, SH), (0, 0, 0, 0))
        d = ImageDraw.Draw(strip)
        widths = [d.textlength(t, font=f) for t, f, c in segs]
        total = sum(widths)
        pad_x, pad_y = 66, 26
        bw, bh = total + pad_x * 2, 46 + pad_y * 2 + 12
        bx, by = SW / 2 - bw / 2, SH / 2 - bh / 2
        d.rounded_rectangle([bx, by, bx + bw, by + bh], radius=30,
                            fill=(13, 15, 20, 232), outline=(255, 255, 255, 30), width=2)
        draw_rich_center(d, (SW / 2, SH / 2 + 1), segs, gap=0)
        strip.save(os.path.join(OUT, f"sub_{i}.png"))
        print(f"sub_{i}.png")


if __name__ == "__main__":
    gen_open()
    gen_end()
    gen_feats()
    gen_subs()
    print("ALL DONE ->", OUT)
