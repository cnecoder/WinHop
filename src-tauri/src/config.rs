use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct Config {
    pub hotkey: String,
    #[serde(default = "default_true")]
    pub elevate: bool,
    pub programs: Vec<Program>,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize, Clone)]
pub struct Program {
    pub key: String,
    pub name: String,
    pub process: String,
}

pub fn load() -> Config {
    let exe_dir = std::env::current_exe().ok().and_then(|p| {
        p.parent().map(|d| d.to_path_buf())
    });
    let cwd = std::env::current_dir().ok();
    let mut candidates: Vec<std::path::PathBuf> = vec![];
    if let Some(d) = exe_dir {
        candidates.push(d.join("config.json"));
    }
    if let Some(d) = cwd {
        candidates.push(d.join("config.json"));
        if let Some(parent) = d.parent() {
            // dev 模式下 tauri CLI 在 src-tauri 下运行 cargo，配置放项目根目录
            candidates.push(parent.join("config.json"));
            if let Some(grand) = parent.parent() {
                // 直接从 target/ 下运行 exe 时再上一级
                candidates.push(grand.join("config.json"));
            }
        }
    }
    for path in candidates {
        if path.exists() {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("读取 {} 失败: {}", path.display(), e));
            let mut cfg: Config = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("解析 {} 失败: {}", path.display(), e));
            validate(&mut cfg);
            // 枚举结果统一小写，配置进程名同样归一化，避免大小写不匹配
            for p in &mut cfg.programs {
                p.process = p.process.to_lowercase();
            }
            return cfg;
        }
    }
    panic!("找不到 config.json（已查找 exe 目录、当前目录、项目根目录）");
}

fn validate(cfg: &mut Config) {
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
