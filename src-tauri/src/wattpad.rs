//! Wattpad : une histoire entière d'après son lien (page de l'histoire ou d'un chapitre).
//! L'API publique du site donne la liste des chapitres, puis le texte de chacun, page par
//! page, en HTML. Les chapitres payants ne sont pas lisibles : ils sont laissés de côté.

use crate::i18n::t;
use crate::link::{decode_entities, BookPart, LinkInfo};
use crate::media::ImportEvent;
use anyhow::{anyhow, Result};
use futures_util::{stream, StreamExt};
use serde_json::Value;

const API: &str = "https://www.wattpad.com";
/// chapitres lus en même temps : assez pour aller vite, sans peser sur le site
const AT_ONCE: usize = 4;
/// pages d'un chapitre au plus (un long chapitre en compte une dizaine)
const MAX_PAGES: usize = 60;

pub fn is_wattpad(host: &str) -> bool {
    host == "wattpad.com" || host.ends_with(".wattpad.com")
}

/// Ce que désigne le lien : une histoire (`/story/123-titre`) ou un chapitre (`/456-titre`).
enum Target {
    Story(String),
    Part(String),
}

fn target(url: &reqwest::Url) -> Option<Target> {
    let mut segs = url.path_segments()?.filter(|s| !s.is_empty());
    let first = segs.next()?;
    let id = |s: &str| {
        let d: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        (!d.is_empty()).then_some(d)
    };
    if first == "story" {
        return id(segs.next()?).map(Target::Story);
    }
    id(first).map(Target::Part)
}

async fn json(c: &reqwest::Client, url: &str) -> Result<Value> {
    let resp = c.get(url).send().await?;
    if resp.status().as_u16() == 404 {
        return Err(anyhow!(t("Cette histoire Wattpad est introuvable (supprimée ou privée).", "This Wattpad story can't be found (deleted or private).")));
    }
    Ok(resp.error_for_status()?.json().await?)
}

/// Le texte d'un chapitre : un paragraphe par `<p>`, sans images ni mise en forme.
fn plain(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for block in html.split("</p>") {
        let mut text = String::with_capacity(block.len());
        let mut inside = false;
        let mut rest = block;
        while let Some(i) = rest.find('<') {
            if !inside {
                text.push_str(&rest[..i]);
            }
            let tag = &rest[i..];
            // un retour à la ligne dans un paragraphe reste un retour à la ligne
            if tag.starts_with("<br") {
                text.push('\n');
            }
            match tag.find('>') {
                Some(j) => rest = &tag[j + 1..],
                None => {
                    inside = true;
                    rest = "";
                }
            }
        }
        if !inside {
            text.push_str(rest);
        }
        let text = decode_entities(&text).replace('\u{a0}', " ");
        let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        if !lines.is_empty() {
            out.push(lines.join("\n"));
        }
    }
    out
}

async fn part_text(c: &reqwest::Client, id: &str) -> Result<String> {
    let mut paras = Vec::new();
    for page in 1..=MAX_PAGES {
        let html = c.get(format!("{API}/apiv2/storytext?id={id}&page={page}")).send().await?.error_for_status()?.text().await?;
        if html.trim().is_empty() {
            break;
        }
        paras.extend(plain(&html));
    }
    Ok(paras.join("\n\n"))
}

pub async fn story(c: &reqwest::Client, url: &reqwest::Url, on_event: &mut (dyn FnMut(ImportEvent) + Send)) -> Result<Option<LinkInfo>> {
    let Some(target) = target(url) else { return Ok(None) };
    let id = match target {
        Target::Story(id) => id,
        Target::Part(part) => {
            let v = json(c, &format!("{API}/api/v3/story_parts/{part}?fields=groupId")).await?;
            match &v["groupId"] {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return Ok(None),
            }
        }
    };
    let v = json(c, &format!("{API}/api/v3/stories/{id}?fields=id,title,cover,url,user(name),parts(id,title,deleted,isBlocked)")).await?;
    let title = v["title"].as_str().unwrap_or("").trim().to_string();
    let parts: Vec<(String, String)> = v["parts"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|p| !p["deleted"].as_bool().unwrap_or(false) && !p["isBlocked"].as_bool().unwrap_or(false))
                .filter_map(|p| {
                    let id = match &p["id"] {
                        Value::Number(n) => n.to_string(),
                        Value::String(s) => s.clone(),
                        _ => return None,
                    };
                    Some((id, p["title"].as_str().unwrap_or("").trim().to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    if parts.is_empty() {
        return Err(anyhow!(t("Cette histoire Wattpad n'a aucun chapitre lisible.", "This Wattpad story has no readable chapters.")));
    }
    on_event(ImportEvent::Stage { stage: "chapters".into() });
    on_event(ImportEvent::Progress { value: 0.0 });
    let total = parts.len();
    let mut texts: Vec<Option<String>> = vec![None; total];
    let mut done = 0;
    // valeurs possédées : le futur reste `Send` pour la commande
    let ids: Vec<(usize, String, reqwest::Client)> = parts.iter().enumerate().map(|(i, (pid, _))| (i, pid.clone(), c.clone())).collect();
    let mut jobs = stream::iter(ids.into_iter().map(|(i, pid, c)| async move { (i, part_text(&c, &pid).await) })).buffer_unordered(AT_ONCE);
    while let Some((i, r)) = jobs.next().await {
        // un chapitre illisible (payant, retiré) n'empêche pas les autres
        texts[i] = r.ok().filter(|s| !s.trim().is_empty());
        done += 1;
        on_event(ImportEvent::Progress { value: done as f64 / total as f64 });
    }
    drop(jobs);
    let chapters: Vec<BookPart> = parts
        .into_iter()
        .zip(texts)
        .enumerate()
        .filter_map(|(i, ((_, name), text))| {
            let name = if name.is_empty() { format!("{} {}", t("Chapitre", "Chapter"), i + 1) } else { name };
            text.map(|text| BookPart { title: name, text })
        })
        .collect();
    if chapters.is_empty() {
        return Err(anyhow!(t(
            "Lumen n'a pu lire aucun chapitre de cette histoire (chapitres payants ou réservés aux abonnés ?).",
            "Lumen couldn't read any chapter of this story (paid or subscriber-only chapters?)."
        )));
    }
    Ok(Some(LinkInfo {
        url: v["url"].as_str().map(str::to_string).unwrap_or_else(|| url.to_string()),
        title,
        site: v["user"]["name"].as_str().map(|n| format!("Wattpad · {n}")).unwrap_or_else(|| "Wattpad".into()),
        image: v["cover"].as_str().unwrap_or("").to_string(),
        chapters,
        ..Default::default()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_text() {
        let u = |s: &str| reqwest::Url::parse(s).unwrap();
        assert!(matches!(target(&u("https://www.wattpad.com/story/401521081-amor-roto")), Some(Target::Story(id)) if id == "401521081"));
        assert!(matches!(target(&u("https://www.wattpad.com/1576249395-amor-roto-divorcio?x=1")), Some(Target::Part(id)) if id == "1576249395"));
        assert!(target(&u("https://www.wattpad.com/user/someone")).is_none());
        let html = r#"<p data-p-id="a" style="x"><b>Primera</b> parte</p><p data-p-id="b"><br></p><p>Uno&nbsp;&amp; dos<br>tres <img src="x.jpg"></p>"#;
        assert_eq!(plain(html), vec!["Primera parte".to_string(), "Uno & dos\ntres".to_string()]);
    }

    /// Lecture réelle d'une histoire : `LUMEN_WATTPAD=<lien> cargo test --lib wattpad_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn wattpad_live() {
        let link = std::env::var("LUMEN_WATTPAD").expect("LUMEN_WATTPAD");
        let c = crate::link::client().unwrap();
        let info = story(&c, &reqwest::Url::parse(&link).unwrap(), &mut |_| {}).await.unwrap().unwrap();
        println!("{} · {} · {} chapitres", info.title, info.site, info.chapters.len());
        for ch in &info.chapters {
            println!("  {} : {} mots", ch.title, ch.text.split_whitespace().count());
        }
    }
}
