//! Outils externes gérés par Lumen : yt-dlp (téléchargement YouTube) et un
//! petit moteur JavaScript (QuickJS) dont yt-dlp a besoin pour YouTube.
//! On utilise ceux du système s'ils existent, sinon Lumen les télécharge
//! une fois dans son dossier de données et les tient à jour.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

pub fn tools_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("tools")
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// Emplacements usuels des outils installés par Homebrew ou à la main
/// (une app lancée depuis le Finder n'hérite pas du PATH du Terminal).
fn system_candidates(name: &str) -> Vec<PathBuf> {
    if cfg!(windows) {
        return windows_candidates(name);
    }
    let h = home();
    vec![
        PathBuf::from(format!("/opt/homebrew/bin/{name}")),
        PathBuf::from(format!("/usr/local/bin/{name}")),
        PathBuf::from(format!("{h}/.homebrew/bin/{name}")),
        PathBuf::from(format!("{h}/.local/bin/{name}")),
        PathBuf::from(format!("{h}/.deno/bin/{name}")),
        PathBuf::from(format!("/usr/bin/{name}")),
    ]
}

/// Windows : outils installés par winget, Scoop, Chocolatey, l'installateur de
/// Node ou celui de Deno, puis les dossiers du PATH (une app Windows en hérite).
fn windows_candidates(name: &str) -> Vec<PathBuf> {
    let exe = format!("{name}.exe");
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut out = Vec::new();
    if let Some(p) = env("LOCALAPPDATA") {
        out.push(p.join("Microsoft").join("WinGet").join("Links").join(&exe));
    }
    if let Some(h) = env("USERPROFILE") {
        out.push(h.join("scoop").join("shims").join(&exe));
        out.push(h.join(".deno").join("bin").join(&exe));
    }
    if let Some(p) = env("ProgramData") {
        out.push(p.join("chocolatey").join("bin").join(&exe));
    }
    if let Some(p) = env("ProgramFiles") {
        out.push(p.join("nodejs").join(&exe));
    }
    if let Some(path) = std::env::var_os("PATH") {
        out.extend(std::env::split_paths(&path).map(|d| d.join(&exe)));
    }
    out
}

fn ytdlp_asset() -> &'static str {
    if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp_linux"
    }
}

fn qjs_asset() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "qjs-darwin-arm64"
    } else if cfg!(target_os = "macos") {
        "qjs-darwin-x86_64"
    } else if cfg!(windows) {
        "qjs-windows-x86_64.exe"
    } else {
        "qjs-linux-x86_64"
    }
}

fn managed_ytdlp(data_dir: &Path) -> PathBuf {
    tools_dir(data_dir).join(if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" })
}

fn managed_qjs(data_dir: &Path) -> PathBuf {
    tools_dir(data_dir).join(if cfg!(windows) { "qjs.exe" } else { "qjs" })
}

/// yt-dlp disponible (système ou géré par Lumen). Sur iPhone et iPad, YouTube passe
/// par Lumen lui-même (ytnative.rs) : ce chemin n'est jamais lancé.
pub fn find_ytdlp(data_dir: &Path) -> Option<PathBuf> {
    if crate::ytnative::is_on() {
        return Some(PathBuf::from("youtube"));
    }
    let m = managed_ytdlp(data_dir);
    if m.exists() {
        return Some(m);
    }
    // une copie du système trop ancienne (installée par pip, jamais mise à jour) fait
    // échouer YouTube (403) : Lumen télécharge alors la sienne, qu'il tient à jour
    system_candidates("yt-dlp").into_iter().find(|p| p.exists() && is_recent(p))
}

/// Une copie de yt-dlp plus vieille que ça ne suit plus YouTube.
const STALE_DAYS: i64 = 45;

/// La version de yt-dlp (« 2026.10.21 »), gardée tant que le fichier ne change pas.
fn version_of(path: &Path) -> Option<String> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashMap<PathBuf, (Option<SystemTime>, Option<String>)>>> = OnceLock::new();
    let stamp = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let seen = SEEN.get_or_init(Default::default);
    if let Some((s, v)) = seen.lock().ok()?.get(path) {
        if *s == stamp {
            return v.clone();
        }
    }
    let v = crate::proc::command(path)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::trim).rfind(|l| days_of(l).is_some()).map(str::to_string));
    seen.lock().ok()?.insert(path.to_path_buf(), (stamp, v.clone()));
    v
}

/// Jours depuis 1970 d'une version datée de yt-dlp (« 2026.10.21 », « 2026.10.21.123456 »).
fn days_of(version: &str) -> Option<i64> {
    let mut parts = version.split('.');
    let (y, m, d) = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    let date = chrono::NaiveDate::from_ymd_opt(y, m, d)?;
    Some(date.signed_duration_since(chrono::NaiveDate::from_ymd_opt(1970, 1, 1)?).num_days())
}

fn is_recent(path: &Path) -> bool {
    let today = chrono::Local::now().date_naive().signed_duration_since(chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()).num_days();
    version_of(path).and_then(|v| days_of(&v)).is_some_and(|d| today - d <= STALE_DAYS)
}

/// Version majeure de Node (yt-dlp exige Node 22 ou plus).
fn node_major(path: &Path) -> Option<u32> {
    let out = crate::proc::command(path).arg("--version").output().ok()?;
    let v = String::from_utf8_lossy(&out.stdout);
    v.trim().trim_start_matches('v').split('.').next()?.parse().ok()
}

/// Argument --js-runtimes à passer à yt-dlp, s'il existe un moteur utilisable.
pub fn js_runtime_arg(data_dir: &Path) -> Option<String> {
    if let Some(d) = system_candidates("deno").into_iter().find(|p| p.exists()) {
        return Some(format!("deno:{}", d.display()));
    }
    for n in system_candidates("node") {
        if n.exists() && node_major(&n).unwrap_or(0) >= 22 {
            return Some(format!("node:{}", n.display()));
        }
    }
    let q = managed_qjs(data_dir);
    if q.exists() {
        return Some(format!("quickjs:{}", q.display()));
    }
    None
}

async fn fetch(url: &str, dest: &Path, mut on_progress: impl FnMut(u64, u64)) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("Lumen/0.1")
        .connect_timeout(Duration::from_secs(20))
        .build()?;
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(anyhow!(crate::tr!("téléchargement refusé ({})", "download refused ({})", resp.status())));
    }
    let total = resp.content_length().unwrap_or(0);
    let tmp = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut got = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        got += chunk.len() as u64;
        on_progress(got, total);
    }
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&tmp, dest).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// Installe (si besoin) yt-dlp et le moteur JavaScript. Renvoie le chemin de yt-dlp.
pub async fn ensure_youtube_tools(data_dir: &Path, mut on_progress: impl FnMut(f64)) -> Result<PathBuf> {
    if crate::ytnative::is_on() {
        return Ok(PathBuf::from("youtube"));
    }
    tokio::fs::create_dir_all(tools_dir(data_dir)).await?;
    let need_ytdlp = find_ytdlp(data_dir).is_none();
    let need_js = js_runtime_arg(data_dir).is_none();
    if need_js {
        let url = format!("https://github.com/quickjs-ng/quickjs/releases/latest/download/{}", qjs_asset());
        fetch(&url, &managed_qjs(data_dir), |g, t| {
            if t > 0 {
                on_progress((g as f64 / t as f64) * if need_ytdlp { 8.0 } else { 100.0 });
            }
        })
        .await
        .map_err(|e| anyhow!(crate::tr!("moteur JavaScript : {e}", "JavaScript engine: {e}")))?;
    }
    if need_ytdlp {
        let url = format!("https://github.com/yt-dlp/yt-dlp/releases/latest/download/{}", ytdlp_asset());
        let base = if need_js { 8.0 } else { 0.0 };
        let got = fetch(&url, &managed_ytdlp(data_dir), |g, t| {
            if t > 0 {
                on_progress(base + (g as f64 / t as f64) * (100.0 - base));
            }
        })
        .await;
        if let Err(e) = got {
            // hors ligne : une copie ancienne du système vaut mieux que rien
            return system_candidates("yt-dlp").into_iter().find(|p| p.exists()).ok_or_else(|| anyhow!("yt-dlp : {e}"));
        }
        let _ = std::fs::write(tools_dir(data_dir).join(".ytdlp-updated"), b"");
    } else {
        maybe_self_update(data_dir).await;
    }
    find_ytdlp(data_dir).ok_or_else(|| anyhow!(crate::i18n::t("yt-dlp est introuvable", "yt-dlp can't be found")))
}

/// YouTube change souvent : la copie gérée de yt-dlp est comparée à la dernière
/// version au plus une fois par jour (un essai manqué se refait trois heures plus tard).
async fn maybe_self_update(data_dir: &Path) {
    let age = |name: &str| {
        std::fs::metadata(tools_dir(data_dir).join(name))
            .and_then(|md| md.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
    };
    let fresh = |name: &str, secs: u64| age(name).is_some_and(|d| d < Duration::from_secs(secs));
    if fresh(".ytdlp-updated", 24 * 3600) || fresh(".ytdlp-tried", 3 * 3600) {
        return;
    }
    let _ = update_ytdlp(data_dir).await;
}

/// Met la copie gérée de yt-dlp à la dernière version (téléchargée par Lumen, qui
/// passe par les certificats du système : le `-U` de yt-dlp échoue derrière un
/// antivirus qui inspecte les connexions). Renvoie vrai si une nouvelle copie est en place.
pub async fn update_ytdlp(data_dir: &Path) -> bool {
    if crate::ytnative::is_on() {
        return false;
    }
    let dir = tools_dir(data_dir);
    let _ = tokio::fs::create_dir_all(&dir).await;
    let _ = std::fs::write(dir.join(".ytdlp-tried"), b"");
    let m = managed_ytdlp(data_dir);
    let run = async {
        // la dernière version, d'après l'adresse vers laquelle GitHub renvoie
        let client = reqwest::Client::builder().user_agent("Lumen").connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(30)).build()?;
        let resp = client.get("https://github.com/yt-dlp/yt-dlp/releases/latest").send().await?;
        let latest = resp.url().path_segments().and_then(|mut s| s.next_back()).unwrap_or("").to_string();
        if m.exists() && !latest.is_empty() && version_of(&m).as_deref() == Some(latest.as_str()) {
            return Ok::<bool, anyhow::Error>(false);
        }
        let url = format!("https://github.com/yt-dlp/yt-dlp/releases/latest/download/{}", ytdlp_asset());
        tokio::time::timeout(Duration::from_secs(180), fetch(&url, &m, |_, _| {})).await.map_err(|_| anyhow!("délai dépassé"))??;
        Ok(true)
    };
    match run.await {
        Ok(changed) => {
            let _ = std::fs::write(dir.join(".ytdlp-updated"), b"");
            changed
        }
        // fichier occupé par un autre téléchargement, réseau coupé : on réessaiera
        Err(_) => false,
    }
}

/// Une erreur de yt-dlp qui vient le plus souvent d'une version dépassée.
pub fn looks_outdated(e: &str) -> bool {
    let m = e.to_lowercase();
    m.contains("403") || m.contains("forbidden") || m.contains("older than") || m.contains("requested format is not available") || m.contains("unable to extract") || m.contains("nsig")
}

pub fn status(data_dir: &Path) -> (bool, bool) {
    if crate::ytnative::is_on() {
        return (true, true);
    }
    (find_ytdlp(data_dir).is_some(), js_runtime_arg(data_dir).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_and_outdated_errors() {
        assert_eq!(days_of("1970.01.02"), Some(1));
        assert!(days_of("2026.10.21.123456").is_some());
        assert_eq!(days_of("Lumen"), None);
        assert!(looks_outdated("unable to download video data: HTTP Error 403: Forbidden"));
        assert!(looks_outdated("Your yt-dlp version (2026.07.04) is older than 90 days!"));
        assert!(!looks_outdated("Video unavailable. This video is private"));
    }
}
