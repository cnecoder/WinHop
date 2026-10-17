"""诊断：找出 hwnd 0x10a84 对应的窗口/进程，检查其提权状态与窗口样式。"""
import ctypes
from ctypes import wintypes

u = ctypes.windll.user32
k = ctypes.windll.kernel32

TARGET = 0x10A84

# 找到目标 hwnd 当前是否仍存在、属于哪个进程
pid = wintypes.DWORD()
u.GetWindowThreadProcessId(TARGET, ctypes.byref(pid))
print(f"hwnd=0x{TARGET:x} pid={pid.value}")

# 进程名与提权状态
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
h = k.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid.value)
if h:
    buf = ctypes.create_unicode_buffer(1024)
    ctypes.windll.psapi.GetModuleFileNameExW(h, None, buf, 1024)
    print("exe:", buf.value)
    tok = wintypes.HANDLE()
    advapi = ctypes.windll.advapi32
    if advapi.OpenProcessToken(h, 0x0008, ctypes.byref(tok)):  # TOKEN_QUERY
        elev = wintypes.DWORD()
        retLen = wintypes.DWORD()
        # TokenElevation = 20
        if advapi.GetTokenInformation(tok, 20, ctypes.byref(elev), 4, ctypes.byref(retLen)):
            print("elevated:", bool(elev.value))
        k.CloseHandle(tok)
    k.CloseHandle(h)
else:
    print("OpenProcess 失败 err=", k.GetLastError())

# 窗口样式
is_iconic = u.IsIconic(TARGET)
is_visible = u.IsWindowVisible(TARGET)
ex_style = u.GetWindowLongW(TARGET, -20)  # GWL_EXSTYLE
print(f"iconic={is_iconic} visible={is_visible} ex_style=0x{ex_style & 0xFFFFFFFF:08x}")
print("WS_EX_TOOLWINDOW" if ex_style & 0x80 else "not toolwindow")

# 顺带列出当前所有可见顶层窗口及其提权状态，便于对照
print("\n--- 可见顶层窗口（前 25 个）---")
EnumWindowsProc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
count = 0

def cb(hwnd, lparam):
    global count
    if count >= 25:
        return True
    if not u.IsWindowVisible(hwnd):
        return True
    if u.IsIconic(hwnd):
        pass  # 也列出最小化的
    p = wintypes.DWORD()
    u.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
    title = ctypes.create_unicode_buffer(128)
    u.GetWindowTextW(hwnd, title, 128)
    hh = k.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, p.value)
    elev = "?"
    name = "?"
    if hh:
        b = ctypes.create_unicode_buffer(512)
        ctypes.windll.psapi.GetModuleFileNameExW(hh, None, b, 512)
        name = b.value.rsplit("\\", 1)[-1]
        tok = wintypes.HANDLE()
        if advapi.OpenProcessToken(hh, 0x0008, ctypes.byref(tok)):
            e = wintypes.DWORD()
            rl = wintypes.DWORD()
            if advapi.GetTokenInformation(tok, 20, ctypes.byref(e), 4, ctypes.byref(rl)):
                elev = "admin" if e.value else "user"
            k.CloseHandle(tok)
        k.CloseHandle(hh)
    flag = " <== 目标" if hwnd == TARGET else ""
    print(f"hwnd=0x{hwnd:x} pid={p.value} {name} [{elev}] iconic={u.IsIconic(hwnd)} {title.value[:40]}{flag}")
    count += 1
    return True

u.EnumWindows(EnumWindowsProc(cb), 0)
