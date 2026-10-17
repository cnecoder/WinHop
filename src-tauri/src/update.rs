//! 检查更新：查询 GitHub Releases 最新版，语义化版本比较后返回结果，
//! 引导用户打开发布页下载（不做应用内下载/安装——那需要签名与更新源基建）。

use serde::Serialize;

const RELEASES_API: &str = "https://api.github.com/repos/cnecoder/WinHop/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/cnecoder/WinHop/releases/latest";

#[derive(Serialize, Clone)]
pub struct UpdateInfo {
    /// 当前运行版本
    pub current: String,
    /// 最新发布版本（去掉 v 前缀）
    pub latest: String,
    pub has_update: bool,
    /// 发布页链接（发现新版时非空）
    pub url: String,
}

/// 解析版本号为 (major, minor, patch)。容忍 v 前缀与 -beta/+build 后缀；
/// 缺省段补 0；非法输入返回 None。
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let core = s.trim().trim_start_matches(['v', 'V']);
    let core = core.split(['-', '+']).next()?;
    let mut it = core.split('.');
    let major = it.next()?.trim().parse().ok()?;
    let minor = it.next()?.trim().parse().ok()?;
    let patch = it.next().map(|p| p.trim().parse().ok()).unwrap_or(Some(0))?;
    Some((major, minor, patch))
}

/// 最新版是否比当前版新（无法解析时保守返回 false）
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

// 查询 GitHub Releases 最新发布（非 draft/prerelease），返回 (版本号, 发布页链接)
fn fetch_latest() -> Result<(String, String), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    let text = agent
        .get(RELEASES_API)
        .set("User-Agent", "WinHop-Update-Check")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| {
            // 静默检查（设置页打开时）失败原本完全无痕迹，排查卡顿问题时留一条日志
            eprintln!("[winhop] 检查更新请求失败: {}（10s 超时内不影响使用）", e);
            format!("网络请求失败: {}", e)
        })?
        .into_string()
        .map_err(|e| format!("读取响应失败: {}", e))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("解析响应失败: {}", e))?;
    // rate-limit / 网络失败时 GitHub 返回 message 字段
    if let Some(msg) = v["message"].as_str() {
        return Err(format!("GitHub API: {}", msg));
    }
    let tag = v["tag_name"]
        .as_str()
        .ok_or_else(|| "响应缺少 tag_name".to_string())?
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();
    let url = v["html_url"]
        .as_str()
        .unwrap_or(RELEASES_PAGE)
        .to_string();
    Ok((tag, url))
}

pub fn check() -> Result<UpdateInfo, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let (latest, url) = fetch_latest()?;
    let has_update = is_newer(&latest, &current);
    Ok(UpdateInfo {
        current,
        latest,
        has_update,
        url: if has_update { url } else { String::new() },
    })
}

/// 设置页「检查更新」命令。必须 async：ureq 同步请求最长挂 10s（网络不通时），
/// 若以同步命令在主线程执行，会阻塞 WebView IPC 与鼠标 LL 钩子（钩子装在主线程）——
/// 实测表现为进设置页后鼠标卡顿数秒、期间点击误判为「点击外部」。
/// 网络 IO 放 spawn_blocking 线程池，主线程只 await 结果。
#[tauri::command]
pub async fn check_update() -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(check)
        .await
        .map_err(|e| format!("检查更新任务失败: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_tolerates_prefix_and_suffix() {
        assert_eq!(parse_version("v0.4.0"), Some((0, 4, 0)));
        assert_eq!(parse_version("0.4.0"), Some((0, 4, 0)));
        assert_eq!(parse_version("V1.2.3-beta.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("2.0"), Some((2, 0, 0)));
        assert_eq!(parse_version("2.0.1+build.5"), Some((2, 0, 1)));
        assert_eq!(parse_version("abc"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn is_newer_compares_semantically() {
        assert!(is_newer("v0.5.0", "0.4.0"));
        assert!(is_newer("0.4.1", "0.4.0"));
        assert!(is_newer("1.0.0-beta", "0.9.9"));
        assert!(!is_newer("0.4.0", "v0.4.0"));
        assert!(!is_newer("0.3.9", "0.4.0"));
        assert!(!is_newer("garbage", "0.4.0"));
    }

    // 真实网络端到端验证：cargo test check_live -- --ignored
    #[test]
    #[ignore]
    fn check_live_reaches_github() {
        let info = check().expect("check 应成功（需可访问 api.github.com）");
        eprintln!(
            "current={} latest={} has_update={} url={}",
            info.current, info.latest, info.has_update, info.url
        );
        assert!(!info.latest.is_empty());
        // 已是最新时不带链接；有新版时必带发布页链接
        assert_eq!(info.url.is_empty(), !info.has_update);
    }
}
