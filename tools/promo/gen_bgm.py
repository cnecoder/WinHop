# -*- coding: utf-8 -*-
"""WinHop 宣传视频 BGM 合成（numpy）
柔和电子氛围：A 小调 pad 和弦 + 低音脉冲 + 轻节奏 + 点缀琶音。
输出 28s 立体声 WAV。
"""
import numpy as np
import wave
import os

SR = 44100
DUR = 62.0
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "assets", "bgm.wav")

n = int(SR * DUR)
t = np.arange(n) / SR
audio = np.zeros(n)


def env_ar(t0, t1, att, rel):
    e = np.zeros(n)
    m = (t >= t0) & (t < t1)
    tt = t[m] - t0
    e[m] = np.minimum(tt / att, 1) * np.minimum((t1 - t[m]) / rel, 1)
    return np.clip(e, 0, 1)


# ---- 和弦进行 Am F C G（每段 3.5s，共两轮 = 28s）----
A2, F2, G2 = 110.00, 87.31, 98.00
chords = [
    ([220.00, 261.63, 329.63], A2),        # Am: A3 C4 E4
    ([174.61, 220.00, 261.63], F2),        # F:  F3 A3 C4
    ([261.63, 329.63, 392.00], 130.81),    # C:  C4 E4 G4
    ([196.00, 246.94, 293.66], G2),        # G:  G3 B3 D4
]
seg = 3.5

# 1) Pad
for i in range(18):
    t0, t1 = i * seg, (i + 1) * seg
    notes, _ = chords[i % 4]
    e = env_ar(t0, t1, 0.7, 0.5)
    for f in notes:
        audio += 0.055 * np.sin(2 * np.pi * f * t) * e
        audio += 0.018 * np.sin(2 * np.pi * f * 1.004 * t) * e

# 2) Bass（每 0.75s 根音脉冲）
tt = 3.2
while tt < 56:
    _, root = chords[int(tt / seg) % 4]
    m = (t >= tt) & (t < tt + 0.7)
    x = t[m] - tt
    audio[m] += 0.15 * np.sin(2 * np.pi * root * t[m]) * np.exp(-x * 3.2)
    tt += 0.75

# 3) Kick（每 1.5s 一记）
tt = 3.4
while tt < 56:
    m = (t >= tt) & (t < tt + 0.35)
    x = t[m] - tt
    audio[m] += 0.20 * np.sin(2 * np.pi * 48.0 * x) * np.exp(-x * 10)
    tt += 1.5

# 4) Hihat（8 分音符，交替强弱，差分白噪做高通）
rng = np.random.default_rng(7)
tt = 3.2 + 0.1875
k = 0
while tt < 56:
    m = (t >= tt) & (t < tt + 0.09)
    x = t[m] - tt
    noise = rng.standard_normal(len(x))
    noise = np.diff(noise, prepend=0.0)  # 简易高通
    amp = 0.035 if k % 2 == 0 else 0.018
    audio[m] += amp * noise * np.exp(-x * 55)
    tt += 0.375
    k += 1

# 5) 琶音点缀（0.75s 一拍，和弦音高八度随机）
rng2 = np.random.default_rng(11)
tt = 6.0
while tt < 54:
    notes, _ = chords[int(tt / seg) % 4]
    f = notes[rng2.integers(0, 3)] * 2
    m = (t >= tt) & (t < tt + 0.6)
    x = t[m] - tt
    audio[m] += 0.042 * np.sin(2 * np.pi * f * t[m]) * np.exp(-x * 5.5)
    tt += 0.75

# ---- 后处理 ----
audio -= audio.mean()
audio = audio / np.max(np.abs(audio)) * 0.72

fade = np.ones(n)
fi, fo = int(1.2 * SR), int(1.8 * SR)
fade[:fi] = np.linspace(0, 1, fi)
fade[-fo:] = np.linspace(1, 0, fo)
audio *= fade

left = audio
right = np.roll(audio, int(0.0006 * SR))  # 0.6ms Haas 微差
stereo = np.stack([left, right], axis=1)
buf = (np.clip(stereo, -1, 1) * 32767).astype(np.int16)

with wave.open(OUT, "wb") as w:
    w.setnchannels(2)
    w.setsampwidth(2)
    w.setframerate(SR)
    w.writeframes(buf.tobytes())

print("bgm.wav", DUR, "s ->", OUT)
