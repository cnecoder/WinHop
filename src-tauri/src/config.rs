use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 窗口排序：Zorder=按创建顺序（默认，稳定）；Mru=最近使用优先（上次用的排 1）
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WindowOrder {
    #[default]
    Zorder,
    Mru,
}

impl WindowOrder {
    pub fn as_str(self) -> &'static str {
        match self {
            WindowOrder::Zorder => "zorder",
            WindowOrder::Mru => "mru",
        }
    }
    // 解析；非法值回退默认（与历史 validate 行为一致，调用方无需再校验）
    pub fn parse(s: &str) -> Self {
        match s {
            "mru" => WindowOrder::Mru,
            _ => WindowOrder::Zorder,
        }
    }
}

impl Serialize for WindowOrder {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for WindowOrder {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let v = WindowOrder::parse(&s);
        if !s.is_empty() && v.as_str() != s {
            eprintln!("[winhop] 配置 window_order 无效「{}」，回退为 {}", s, v.as_str());
        }
        Ok(v)
    }
}

/// 多字母模式窗口层数字键行为：Jump=按数字直接跳转（默认）；Preview=先聚焦预览、Enter 跳转
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WinDigitMode {
    #[default]
    Jump,
    Preview,
}

impl WinDigitMode {
    pub fn as_str(self) -> &'static str {
        match self {
            WinDigitMode::Jump => "jump",
            WinDigitMode::Preview => "preview",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "preview" => WinDigitMode::Preview,
            _ => WinDigitMode::Jump,
        }
    }
}

impl Serialize for WinDigitMode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for WinDigitMode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let v = WinDigitMode::parse(&s);
        if !s.is_empty() && v.as_str() != s {
            eprintln!("[winhop] 配置 win_digit_mode 无效「{}」，回退为 {}", s, v.as_str());
        }
        Ok(v)
    }
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Config {
    pub hotkey: String,
    #[serde(default = "default_true")]
    pub elevate: bool,
    /// 开机自启：写注册表 HKCU\...\Run（由 windows::set_autostart 落地，非仅配置项）
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub window_order: WindowOrder,
    #[serde(default)]
    pub multi_letter: bool,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// 多字母模式窗口层数字键行为
    #[serde(default)]
    pub win_digit_mode: WinDigitMode,
    /// 界面语言："zh-CN"/"en"，空串为跟随系统（默认）
    #[serde(default)]
    pub lang: String,
    pub programs: Vec<Program>,
    /// 黑名单：命中的程序不进入列表。
    /// 首次/老配置播种 system-blocklist.txt 的系统默认项，之后完全由用户控制（设置页可解除）。
    /// 序列化兼容两种形式：裸字符串 "a.exe"（用户屏蔽，无备注）或 {"process":"a.exe","note":".."}（带备注）
    #[serde(default)]
    pub blocked: Vec<Blocked>,
    /// 黑名单是否已播种系统默认（仅播种一次；用户解除后不再补回）
    #[serde(default)]
    pub blocked_seeded: bool,
}

/// 黑名单条目：process 为小写 exe 名，note 为说明（系统预置项带备注，用户屏蔽项为空）
#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum Blocked {
    Name(String),
    Entry {
        process: String,
        #[serde(default)]
        note: String,
    },
}

impl Blocked {
    pub fn process(&self) -> &str {
        match self {
            Blocked::Name(s) => s,
            Blocked::Entry { process, .. } => process,
        }
    }
    pub fn note(&self) -> &str {
        match self {
            Blocked::Name(_) => "",
            Blocked::Entry { note, .. } => note,
        }
    }
}

// 系统默认黑名单种子（来自随程序分发的 system-blocklist.txt，格式「exe 名 # 备注」）
fn default_blocked() -> Vec<Blocked> {
    include_str!("../system-blocklist.txt")
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let (proc, note) = match l.split_once('#') {
                Some((p, n)) => (p.trim(), n.trim()),
                None => (l, ""),
            };
            let proc = proc.to_lowercase();
            if proc.is_empty() {
                return None;
            }
            Some(Blocked::Entry {
                process: proc,
                note: note.to_string(),
            })
        })
        .collect()
}

fn default_true() -> bool {
    true
}

fn default_theme() -> String {
    "black-green".into()
}

// 最小基线默认：标量取各 default_*，programs/blocked 为空、blocked_seeded=true。
// 首启生成的完整默认配置在此基础上补 default_programs()/default_blocked()（见 load）。
impl Default for Config {
    fn default() -> Self {
        Config {
            hotkey: "ctrl+space".into(),
            elevate: true,
            autostart: false,
            window_order: WindowOrder::default(),
            multi_letter: false,
            theme: default_theme(),
            win_digit_mode: WinDigitMode::default(),
            lang: String::new(),
            programs: Vec::new(),
            blocked: Vec::new(),
            blocked_seeded: true,
        }
    }
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Program {
    /// 单字母模式快捷键（单个小写字母，可为空——仅配置了多字母的程序）
    #[serde(default)]
    pub key: String,
    /// 多字母模式快捷键（1+ 小写字母，可为空）
    #[serde(default)]
    pub multi_key: String,
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
    // 枚举结果统一小写，配置键名/进程名同样归一化，避免大小写不匹配
    for p in &mut cfg.programs {
        p.process = p.process.to_lowercase();
        p.key = p.key.to_lowercase();
        p.multi_key = p.multi_key.to_lowercase();
    }
    for b in &mut cfg.blocked {
        match b {
            Blocked::Name(s) => *s = s.trim().to_lowercase(),
            Blocked::Entry { process, .. } => *process = process.trim().to_lowercase(),
        }
    }
    cfg.blocked.retain(|b| !b.process().is_empty());
    cfg.blocked.sort_by(|a, b| a.process().cmp(b.process()));
    cfg.blocked.dedup_by(|a, b| a.process() == b.process());
    // 首次/老配置（blocked_seeded 缺省为 false）：播种系统默认黑名单一次。
    // 之后完全交给用户——解除即写盘，不再补回。
    if !cfg.blocked_seeded {
        for d in default_blocked() {
            if !cfg.blocked.iter().any(|b| b.process() == d.process()) {
                cfg.blocked.push(d);
            }
        }
        cfg.blocked.sort_by(|a, b| a.process().cmp(b.process()));
        cfg.blocked.dedup_by(|a, b| a.process() == b.process());
        cfg.blocked_seeded = true;
        let _ = save(&cfg, path); // 持久化播种结果，失败不影响运行
    }
    // 给已播种但缺备注的系统项补说明（升级老数据：早期版本播种无备注）
    let sys = default_blocked();
    let sys_notes: std::collections::HashMap<&str, &str> = sys
        .iter()
        .map(|b| (b.process(), b.note()))
        .filter(|(_, n)| !n.is_empty())
        .collect();
    let mut note_fixed = false;
    for b in cfg.blocked.iter_mut() {
        if b.note().is_empty() {
            if let Some(note) = sys_notes.get(b.process()) {
                *b = Blocked::Entry {
                    process: b.process().to_string(),
                    note: (*note).to_string(),
                };
                note_fixed = true;
            }
        }
    }
    if note_fixed {
        let _ = save(&cfg, path);
    }
    cfg
}

// 配置文件放 %APPDATA%\WinHop\config.json：升级/重装安装器不碰用户目录，配置不丢。
// 旧版配置位置：exe 目录/项目根目录，以及改名前的 %APPDATA%\WinTab 目录——首次运行自动迁移。
pub fn load() -> (Config, std::path::PathBuf) {
    let appdata = std::env::var("APPDATA")
        .ok()
        .map(|d| std::path::PathBuf::from(d).join("WinHop"));
    let new_path = appdata.as_ref().map(|d| d.join("config.json"));
    if let Some(p) = new_path.as_ref() {
        if p.exists() {
            eprintln!("[winhop] 加载配置 {}", p.display());
            return (read_cfg(p), p.clone());
        }
    }
    // 改名迁移：%APPDATA%\WinTab 整目录搬到 WinHop（含 config.json 与旧日志），搬完旧目录删除
    if let Some(appdata_base) = std::env::var("APPDATA").ok() {
        let old_dir = std::path::PathBuf::from(&appdata_base).join("WinTab");
        let old_cfg = old_dir.join("config.json");
        if old_cfg.exists() {
            if let Some(new_dir) = appdata.as_ref() {
                if std::fs::create_dir_all(new_dir).is_ok() {
                    // 逐文件复制（目标已存在的文件跳过，不覆盖新配置）
                    if let Ok(entries) = std::fs::read_dir(&old_dir) {
                        for ent in entries.flatten() {
                            let from = ent.path();
                            let to = new_dir.join(ent.file_name());
                            if from.is_file() && !to.exists() {
                                let _ = std::fs::copy(&from, &to);
                            }
                        }
                    }
                    if let Some(new) = new_path.as_ref() {
                        if new.exists() {
                            eprintln!("[winhop] 迁移改名前配置 {} → {}", old_dir.display(), new_dir.display());
                            let _ = std::fs::remove_dir_all(&old_dir);
                            return (read_cfg(new), new.clone());
                        }
                    }
                }
            }
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
                        "[winhop] 迁移旧配置 {} → {}",
                        path.display(),
                        new.display()
                    );
                    return (cfg, new.clone());
                }
            }
            // APPDATA 不可用：退回旧位置（配置仍生效，只是不迁移）
            eprintln!("[winhop] 加载配置 {}（无法迁移到 APPDATA）", path.display());
            return (cfg, path);
        }
    }
    // 找不到配置：在最小基线（Config::default）上补预置别名程序与系统黑名单
    let default = Config {
        programs: default_programs(),
        blocked: default_blocked(),
        ..Config::default()
    };
    let json = serde_json::to_string_pretty(&default).expect("序列化默认配置失败");
    if let Some(dir) = appdata.as_ref() {
        let path = dir.join("config.json");
        if std::fs::create_dir_all(dir).is_ok() && std::fs::write(&path, &json).is_ok() {
            eprintln!("[winhop] 已创建默认配置 {}", path.display());
            return (default, path);
        }
    }
    if let Some(d) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        let path = d.join("config.json");
        if std::fs::write(&path, &json).is_ok() {
            eprintln!("[winhop] 已创建默认配置 {}", path.display());
            return (default, path);
        }
    }
    panic!("找不到且无法创建 config.json");
}

pub fn save(cfg: &Config, path: &std::path::Path) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(cfg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    // 原子写：先写临时文件再 rename，避免崩溃/断电时写坏 config.json 导致下次启动 panic
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

// 预置常用软件别名：key 为单字母模式代号，multi_key 为多字母模式代号，process 为小写 exe 名。
// 用户首次启动即生成；未运行的条目灰色排在末尾，不做启动器。
fn default_programs() -> Vec<Program> {
    let p = |key: &str, mk: &str, name: &str, proc: &str| Program {
        key: key.into(),
        multi_key: mk.into(),
        name: name.into(),
        process: proc.into(),
    };
    vec![
        p("c", "ch", "Chrome", "chrome.exe"),
        p("e", "ed", "Edge", "msedge.exe"),
        p("f", "ff", "Firefox", "firefox.exe"),
        p("v", "vs", "VS Code", "code.exe"),
        p("t", "te", "终端", "windowsterminal.exe"),
        p("p", "ps", "PowerShell", "powershell.exe"),
        p("n", "no", "记事本", "notepad.exe"),
        p("r", "ex", "资源管理器", "explorer.exe"),
        p("s", "sl", "Slack", "slack.exe"),
        p("d", "di", "Discord", "discord.exe"),
        p("o", "ou", "Outlook", "outlook.exe"),
        p("w", "wo", "Word", "winword.exe"),
        p("x", "xl", "Excel", "excel.exe"),
        p("y", "wx", "微信", "wechat.exe"),
        p("q", "qq", "QQ", "qq.exe"),
        p("z", "dd", "钉钉", "dingtalk.exe"),
    ]
}

/// 已知主题 id（与前端 styles.css 的 [data-theme=...] 对应）
pub const THEMES: &[&str] = &["black-green", "black-yellow"];

fn validate(cfg: &mut Config) {
    // window_order / win_digit_mode 为枚举，反序列化时非法值已回退默认（见其 Deserialize）
    if !THEMES.contains(&cfg.theme.as_str()) {
        eprintln!(
            "[winhop] 配置 theme 无效「{}」，回退为 {}",
            cfg.theme,
            THEMES[0]
        );
        cfg.theme = THEMES[0].into();
    }
    // lang：空（跟随系统）/ zh-CN / en，其余归一化为空
    if cfg.lang != "" && cfg.lang != "zh-CN" && cfg.lang != "en" {
        eprintln!("[winhop] 配置 lang 无效「{}」，回退为跟随系统", cfg.lang);
        cfg.lang = String::new();
    }
    let mut seen_key = std::collections::HashSet::new();
    let mut seen_mk = std::collections::HashSet::new();
    for p in &cfg.programs {
        // key：可为空（仅用多字母），非空则必须单小写字母且唯一
        if !p.key.is_empty() {
            if p.key.len() != 1 || !p.key.as_bytes()[0].is_ascii_lowercase() {
                panic!("程序「{}」的单字母代号必须是单个小写字母，当前为 {:?}", p.name, p.key);
            }
            if !seen_key.insert(p.key.clone()) {
                panic!("单字母代号重复: {}", p.key);
            }
        }
        // multi_key：可为空（仅用单字母），非空则全小写字母且唯一
        if !p.multi_key.is_empty() {
            if !p.multi_key.bytes().all(|b| b.is_ascii_lowercase()) {
                panic!("程序「{}」的多字母代号必须全为小写字母，当前为 {:?}", p.name, p.multi_key);
            }
            if !seen_mk.insert(p.multi_key.clone()) {
                panic!("多字母代号重复: {}", p.multi_key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prog(key: &str, mk: &str, name: &str, proc_: &str) -> Program {
        Program {
            key: key.into(),
            multi_key: mk.into(),
            name: name.into(),
            process: proc_.into(),
        }
    }

    fn minimal_cfg() -> Config {
        Config::default() // 字段默认即测试基线；加字段无需改这里
    }

    #[test]
    fn valid_config_passes() {
        let mut cfg = minimal_cfg();
        cfg.programs = vec![
            prog("c", "ch", "Chrome", "chrome.exe"),
            prog("v", "vs", "Code", "code.exe"),
        ];
        validate(&mut cfg); // 不应 panic
    }

    #[test]
    fn invalid_enums_fall_back() {
        // 枚举字段：非法 JSON 值反序列化即回退默认（window_order→zorder、win_digit_mode→jump）
        let cfg: Config = serde_json::from_str(
            r#"{"hotkey":"ctrl+space","programs":[],
                "window_order":"bogus","win_digit_mode":"bogus","theme":"no-such-theme","lang":"fr"}"#,
        )
        .unwrap();
        assert_eq!(cfg.window_order, WindowOrder::Zorder);
        assert_eq!(cfg.win_digit_mode, WinDigitMode::Jump);
        // theme/lang 仍在 validate 中回退
        let mut cfg = cfg;
        validate(&mut cfg);
        assert_eq!(cfg.theme, THEMES[0]);
        assert_eq!(cfg.lang, "");
    }

    #[test]
    fn empty_and_duplicate_keys_panic() {
        // 空 key 允许（仅多字母程序）；重复单字母 panic
        let mut cfg = minimal_cfg();
        cfg.programs = vec![prog("", "ch", "A", "a.exe")];
        validate(&mut cfg); // 空 key 合法

        cfg.programs = vec![prog("c", "", "A", "a.exe"), prog("c", "", "B", "b.exe")];
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| validate(&mut cfg))).is_err());
    }

    #[test]
    fn invalid_single_key_panics() {
        let mut cfg = minimal_cfg();
        cfg.programs = vec![prog("cd", "", "A", "a.exe")]; // 单字母模式代号多字符
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| validate(&mut cfg))).is_err());

        cfg.programs = vec![prog("C", "", "A", "a.exe")]; // 大写
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| validate(&mut cfg))).is_err());
    }

    #[test]
    fn duplicate_or_invalid_multi_key_panics() {
        let mut cfg = minimal_cfg();
        cfg.programs = vec![prog("", "ch", "A", "a.exe"), prog("", "ch", "B", "b.exe")];
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| validate(&mut cfg))).is_err());

        cfg.programs = vec![prog("", "CH", "A", "a.exe")]; // 大写
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| validate(&mut cfg))).is_err());
    }

    #[test]
    fn save_is_atomic_and_roundtrips() {
        let dir = std::env::temp_dir().join(format!("winhop_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let mut cfg = minimal_cfg();
        cfg.programs = vec![prog("c", "ch", "Chrome", "chrome.exe")];
        save(&cfg, &path).unwrap();
        // 原子写：目标存在、临时文件不残留
        assert!(path.exists());
        assert!(!dir.join("config.json.tmp").exists());
        let text = std::fs::read_to_string(&path).unwrap();
        let back: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(back.programs.len(), 1);
        assert_eq!(back.programs[0].process, "chrome.exe");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn default_programs_present() {
        assert!(!default_programs().is_empty());
        // 默认代号单字母唯一
        let mut seen = std::collections::HashSet::new();
        for p in default_programs() {
            assert!(seen.insert(p.key.clone()), "默认单字母代号重复 {}", p.key);
        }
    }

    #[test]
    fn blocked_untagged_deserializes_both_forms() {
        let c: Config = serde_json::from_str(
            r#"{"hotkey":"ctrl+space","programs":[],
                "blocked":["a.exe",{"process":"b.exe","note":"x"}]}"#,
        )
        .unwrap();
        assert_eq!(c.blocked.len(), 2);
        assert_eq!(c.blocked[0].process(), "a.exe");
        assert_eq!(c.blocked[0].note(), "");
        assert_eq!(c.blocked[1].process(), "b.exe");
        assert_eq!(c.blocked[1].note(), "x");
    }
}
