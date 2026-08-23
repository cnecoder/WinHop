use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone)]
pub struct Config {
    pub hotkey: String,
    #[serde(default = "default_true")]
    pub elevate: bool,
    #[serde(default = "default_window_order")]
    pub window_order: String,
    #[serde(default)]
    pub multi_letter: bool,
    pub programs: Vec<Program>,
}

fn default_true() -> bool {
    true
}

fn default_window_order() -> String {
    "zorder".into()
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Program {
    pub key: String,
    pub name: String,
    pub process: String,
}

// 读取并规范化某个路径下的配置文件（解析失败即 panic，不静默）
fn read_cfg(path: &std::path::Path) -> Config {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("读取 {} 失败: {}", path.display(), e));
    let mut cfg: Config = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("解析 {} 失败: {}", path.display(), e));
    validate(&mut cfg);
    // 枚举结果统一小写，配置进程名同样归一化，避免大小写不匹配
    for p in &mut cfg.programs {
        p.process = p.process.to_lowercase();
    }
    cfg
}

// 配置文件放 %APPDATA%\WinTab\config.json：升级/重装安装器不碰用户目录，配置不丢。
// 旧版配置在 exe 目录/项目根目录：首次运行自动迁移（复制到新位置，旧文件保留）。
pub fn load() -> (Config, std::path::PathBuf) {
    let appdata = std::env::var("APPDATA")
        .ok()
        .map(|d| std::path::PathBuf::from(d).join("WinTab"));
    let new_path = appdata.as_ref().map(|d| d.join("config.json"));
    if let Some(p) = new_path.as_ref() {
        if p.exists() {
            eprintln!("[wintab] 加载配置 {}", p.display());
            return (read_cfg(p), p.clone());
        }
    }
    // 旧位置查找（exe 目录 → 当前目录 → 项目根目录），找到则迁移到 APPDATA
    let mut legacy: Vec<std::path::PathBuf> = vec![];
    if let Some(d) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    {
        legacy.push(d.join("config.json"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        legacy.push(cwd.join("config.json"));
        if let Some(parent) = cwd.parent() {
            // dev 模式下 tauri CLI 在 src-tauri 下运行 cargo，配置放项目根目录
            legacy.push(parent.join("config.json"));
            if let Some(grand) = parent.parent() {
                // 直接从 target/ 下运行 exe 时再上一级
                legacy.push(grand.join("config.json"));
            }
        }
    }
    for path in legacy {
        if path.exists() {
            let cfg = read_cfg(&path);
            if let (Some(dir), Some(new)) = (appdata.as_ref(), new_path.as_ref()) {
                if std::fs::create_dir_all(dir).is_ok()
                    && std::fs::copy(&path, new).is_ok()
                {
                    eprintln!(
                        "[wintab] 迁移旧配置 {} → {}",
                        path.display(),
                        new.display()
                    );
                    return (cfg, new.clone());
                }
            }
            // APPDATA 不可用：退回旧位置（配置仍生效，只是不迁移）
            eprintln!("[wintab] 加载配置 {}（无法迁移到 APPDATA）", path.display());
            return (cfg, path);
        }
    }
    // 找不到配置：生成默认配置（常用软件预置别名 + 字母，自动补全补齐其余）
    let default = Config {
        hotkey: "ctrl+space".into(),
        elevate: true,
        window_order: "zorder".into(),
        multi_letter: false,
        programs: default_programs(),
    };
    let json = serde_json::to_string_pretty(&default).expect("序列化默认配置失败");
    if let Some(dir) = appdata.as_ref() {
        let path = dir.join("config.json");
        if std::fs::create_dir_all(dir).is_ok() && std::fs::write(&path, &json).is_ok() {
            eprintln!("[wintab] 已创建默认配置 {}", path.display());
            return (default, path);
        }
    }
    if let Some(d) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        let path = d.join("config.json");
        if std::fs::write(&path, &json).is_ok() {
            eprintln!("[wintab] 已创建默认配置 {}", path.display());
            return (default, path);
        }
    }
    panic!("找不到且无法创建 config.json");
}

pub fn save(cfg: &Config, path: &std::path::Path) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(cfg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, text)
}

// 预置常用软件别名（key 为字母代号，process 为小写 exe 名）。
// 用户首次启动即生成；未运行的条目灰色排在末尾，不做启动器。
fn default_programs() -> Vec<Program> {
    vec![
        Program { key: "c".into(), name: "Chrome".into(), process: "chrome.exe".into() },
        Program { key: "e".into(), name: "Edge".into(), process: "msedge.exe".into() },
        Program { key: "f".into(), name: "Firefox".into(), process: "firefox.exe".into() },
        Program { key: "v".into(), name: "VS Code".into(), process: "code.exe".into() },
        Program { key: "t".into(), name: "终端".into(), process: "windowsterminal.exe".into() },
        Program { key: "p".into(), name: "PowerShell".into(), process: "powershell.exe".into() },
        Program { key: "n".into(), name: "记事本".into(), process: "notepad.exe".into() },
        Program { key: "r".into(), name: "资源管理器".into(), process: "explorer.exe".into() },
        Program { key: "s".into(), name: "Slack".into(), process: "slack.exe".into() },
        Program { key: "d".into(), name: "Discord".into(), process: "discord.exe".into() },
        Program { key: "o".into(), name: "Outlook".into(), process: "outlook.exe".into() },
        Program { key: "w".into(), name: "Word".into(), process: "winword.exe".into() },
        Program { key: "x".into(), name: "Excel".into(), process: "excel.exe".into() },
        Program { key: "y".into(), name: "微信".into(), process: "wechat.exe".into() },
        Program { key: "q".into(), name: "QQ".into(), process: "qq.exe".into() },
        Program { key: "z".into(), name: "钉钉".into(), process: "dingtalk.exe".into() },
    ]
}

fn validate(cfg: &mut Config) {
    if cfg.window_order != "zorder" && cfg.window_order != "mru" {
        eprintln!(
            "[wintab] 配置 window_order 无效「{}」，回退为 zorder",
            cfg.window_order
        );
        cfg.window_order = "zorder".into();
    }
    let mut seen = std::collections::HashSet::new();
    for p in &cfg.programs {
        if p.key.len() != 1 || !p.key.as_bytes()[0].is_ascii_lowercase() {
            panic!("程序「{}」的 key 必须是单个小写字母，当前为 {:?}", p.name, p.key);
        }
        if !seen.insert(p.key.clone()) {
            panic!("字母代号重复: {}", p.key);
        }
    }
}
