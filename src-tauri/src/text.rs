//! Normalisation et découpage du texte en mots (UAX #29), identiques partout
//! dans l'application : la clé d'un mot est sa forme en minuscules, sans
//! accent tonique (russe), avec l'apostrophe typographique unifiée.

use serde::Serialize;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

/// Langues proposées par Lumen. Tagalog a un dictionnaire local, mais n'est
/// pas pris en charge par Supertonic 3 pour la voix naturelle.
pub const LANGS: &[&str] = &[
    "en", "it", "de", "pt", "ru", "es", "fr", "nl", "sv", "da", "fi", "et", "lv", "lt", "pl", "cs", "sk", "sl", "hr", "hu", "ro", "bg", "uk",
    "el", "tr", "ar", "hi", "id", "vi", "ko", "ja", "tl",
];

pub const TTS_LANGS: &[&str] = &[
    "en", "it", "de", "pt", "ru", "es", "fr", "nl", "sv", "da", "fi", "et", "lv", "lt", "pl", "cs", "sk", "sl", "hr", "hu", "ro", "bg", "uk",
    "el", "tr", "ar", "hi", "id", "vi", "ko", "ja",
];

/// Clé de recherche d'un mot ou d'une expression (accents aigu et grave retirés).
pub fn normalize(s: &str) -> String {
    normalize_for(s, "")
}

/// Langues où les accents aigu et grave ne distinguent pas les mots (accent
/// tonique des textes russes, ukrainiens, bulgares…) : on les retire de la clé.
/// Les six premières langues de Lumen gardent ce comportement pour que leur
/// vocabulaire ne change pas. Ailleurs (vietnamien, grec, tchèque, français…),
/// ces accents font partie du mot : « má », « mà » et « ma » restent distincts.
pub fn strips_accents(lang: &str) -> bool {
    matches!(lang, "" | "en" | "it" | "de" | "pt" | "ru" | "es" | "uk" | "bg")
}

/// Clé d'un mot ou d'une expression dans une langue donnée.
pub fn normalize_for(s: &str, lang: &str) -> String {
    let lower = s.trim().to_lowercase().replace('’', "'");
    let kept: String = if strips_accents(lang) {
        lower.nfd().filter(|c| *c != '\u{0301}' && *c != '\u{0300}').nfc().collect()
    } else {
        lower.nfc().collect()
    };
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Serialize, Clone, Debug)]
pub struct Token {
    /// texte tel qu'il apparaît
    pub t: String,
    /// vrai si c'est un mot à apprendre
    pub w: bool,
    /// clé normalisée (vide si ce n'est pas un mot)
    pub k: String,
    /// début et fin en unités UTF-16 (compatibles avec les chaînes JavaScript)
    pub s: usize,
    pub e: usize,
}

/// Ponctuation de fin de phrase (latine, grecque « ; », japonaise, hindi, arabe).
pub fn ends_sentence(piece: &str) -> bool {
    piece.contains(|c: char| matches!(c, '.' | '!' | '?' | '…' | ';' | '。' | '！' | '？' | '।' | '؟' | '\n'))
}

/// Phrases d'un texte, en indices de jetons [début, fin) ; chacune contient au
/// moins un mot. La ponctuation finale et un guillemet fermant restent avec elle.
pub fn sentences(tokens: &[Token]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut a = 0usize;
    let mut i = 0usize;
    while i < tokens.len() {
        if !tokens[i].w && ends_sentence(&tokens[i].t) {
            let mut b = i + 1;
            while b < tokens.len() && !tokens[b].w && !tokens[b].t.contains('\n') && tokens[b].t.trim().chars().all(|c| "»\"”’)]」".contains(c)) {
                b += 1;
            }
            if tokens[a..b].iter().any(|t| t.w) {
                out.push((a, b));
            }
            a = b;
            i = b;
            continue;
        }
        i += 1;
    }
    if a < tokens.len() && tokens[a..].iter().any(|t| t.w) {
        out.push((a, tokens.len()));
    }
    out
}

fn is_learnable(piece: &str) -> bool {
    piece.chars().any(|c| c.is_alphabetic())
}

/// Langues où l'élision (l'uomo, dell'acqua, d'água) doit être séparée.
fn splits_elision(lang: &str) -> bool {
    matches!(lang, "it" | "fr" | "pt" | "ca")
}

fn japanese_word_piece(piece: &str) -> bool {
    piece.chars().any(|c| {
        matches!(
            c,
            '\u{3040}'..='\u{309f}'
                | '\u{30a0}'..='\u{30ff}'
                | '\u{3400}'..='\u{4dbf}'
                | '\u{4e00}'..='\u{9fff}'
                | '\u{f900}'..='\u{faff}'
        )
    })
}

/// TinySegmenter intentionally exposes some inflection boundaries (for
/// example `見` + `た`). Lumen's learner-facing words should remain useful
/// tap targets, so keep common Japanese inflections with their stem.
fn japanese_inflection(piece: &str) -> bool {
    matches!(
        piece,
        "い"
            | "かっ"
            | "き"
            | "く"
            | "け"
            | "げ"
            | "さ"
            | "し"
            | "す"
            | "せ"
            | "そ"
            | "た"
            | "だ"
            | "ち"
            | "っ"
            | "つ"
            | "て"
            | "で"
            | "と"
            | "な"
            | "ない"
            | "なく"
            | "なかっ"
            | "に"
            | "ぬ"
            | "ね"
            | "の"
            | "は"
            | "ひ"
            | "ふ"
            | "へ"
            | "ほ"
            | "まし"
            | "ませ"
            | "ました"
            | "ません"
            | "み"
            | "む"
            | "め"
            | "も"
            | "や"
            | "ゆ"
            | "よ"
            | "ら"
            | "り"
            | "る"
            | "れ"
            | "ろ"
            | "わ"
            | "ん"
            | "れる"
            | "られ"
            | "れば"
            | "ろう"
            | "たい"
            | "たく"
            | "てい"
            | "てく"
            | "でし"
    )
}

fn japanese_hiragana_word(piece: &str) -> bool {
    !piece.is_empty() && piece.chars().all(|c| matches!(c, '\u{3040}'..='\u{309f}'))
}

fn japanese_tokens(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for piece in tiniestsegmenter::tokenize(text) {
        let Some(previous) = out.last_mut() else {
            out.push(piece.to_string());
            continue;
        };

        let attach_adverbial_ni = piece == "に"
            && japanese_hiragana_word(previous)
            && previous.chars().count() > 1
            && previous.ends_with('い');
        let attach_inflection = japanese_inflection(piece)
            && !matches!(piece, "に" | "の" | "は" | "が" | "を" | "で" | "と" | "も");

        if japanese_word_piece(previous)
            && japanese_word_piece(piece)
            && (attach_inflection || attach_adverbial_ni)
        {
            previous.push_str(piece);
        } else {
            out.push(piece.to_string());
        }
    }
    out
}

pub fn tokenize(text: &str, lang: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut utf16_pos = 0usize;
    let push = |piece: &str, out: &mut Vec<Token>, pos: &mut usize| {
        let len16: usize = piece.encode_utf16().count();
        let w = is_learnable(piece);
        out.push(Token {
            t: piece.to_string(),
            w,
            k: if w { normalize_for(piece, lang) } else { String::new() },
            s: *pos,
            e: *pos + len16,
        });
        *pos += len16;
    };

    // Japanese does not put spaces between words. Unicode word boundaries
    // therefore fall back to one token per character; use the same compact
    // TinySegmenter-style lexical tokenizer as Komichi instead.
    if lang == "ja" {
        for piece in japanese_tokens(text) {
            push(&piece, &mut out, &mut utf16_pos);
        }
        return out;
    }

    for piece in text.split_word_bounds() {
        if splits_elision(lang) && is_learnable(piece) {
            if let Some(idx) = piece.find(|c| c == '\'' || c == '’') {
                let (head, rest) = piece.split_at(idx);
                let apos_len = rest.chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                let tail = &rest[apos_len..];
                if !head.is_empty() && head.chars().count() <= 6 && is_learnable(tail) {
                    let head_with_apos = &piece[..idx + apos_len];
                    push(head_with_apos, &mut out, &mut utf16_pos);
                    push(tail, &mut out, &mut utf16_pos);
                    continue;
                }
            }
        }
        push(piece, &mut out, &mut utf16_pos);
    }
    out
}

/// Liste des clés de mots (avec doublons) d'un texte.
pub fn word_keys(text: &str, lang: &str) -> Vec<String> {
    tokenize(text, lang).into_iter().filter(|t| t.w).map(|t| t.k).collect()
}

/// Nombre de mots d'un texte.
pub fn word_count(text: &str, lang: &str) -> usize {
    tokenize(text, lang).iter().filter(|t| t.w).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accents_kept_where_they_matter() {
        // vietnamien : les tons distinguent les mots
        let k: Vec<_> = tokenize("má mà ma", "vi").into_iter().filter(|t| t.w).map(|t| t.k).collect();
        assert_eq!(k, ["má", "mà", "ma"]);
        // russe : l'accent tonique des manuels disparaît de la clé
        assert_eq!(normalize_for("до\u{301}м", "ru"), "дом");
        assert_eq!(normalize_for("Πότε", "el"), "πότε");
    }

    #[test]
    fn sentence_split() {
        let t = tokenize("Hello there. How are you? Fine…\nYes", "en");
        let s: Vec<String> = sentences(&t).iter().map(|(a, b)| t[*a..*b].iter().map(|x| x.t.as_str()).collect::<String>().trim().to_string()).collect();
        assert_eq!(s, ["Hello there.", "How are you?", "Fine…", "Yes"]);
        let j = tokenize("毎朝、海を見た。手紙を見つけた。", "ja");
        assert_eq!(sentences(&j).len(), 2);
    }

    #[test]
    fn elision_italian() {
        let t: Vec<_> = tokenize("Dell'acqua e l'uomo.", "it").into_iter().filter(|t| t.w).map(|t| t.k).collect();
        assert_eq!(t, vec!["dell'", "acqua", "e", "l'", "uomo"]);
    }
    #[test]
    fn english_contraction_kept() {
        let t: Vec<_> = tokenize("Don't stop", "en").into_iter().filter(|t| t.w).map(|t| t.k).collect();
        assert_eq!(t, vec!["don't", "stop"]);
    }
    #[test]
    fn russian_stress() {
        assert_eq!(normalize("до\u{301}м"), "дом");
    }
    #[test]
    fn utf16_offsets() {
        let toks = tokenize("Größe 😀 ok", "de");
        let last = toks.last().unwrap();
        assert_eq!(last.t, "ok");
        assert_eq!(last.s, 9);
    }

    #[test]
    fn japanese_uses_lexical_words() {
        let tokens: Vec<_> = tokenize("毎朝、海を見た。手紙を見つけた。", "ja")
            .into_iter()
            .filter(|t| t.w)
            .map(|t| t.t)
            .collect();
        assert_eq!(tokens, ["毎朝", "海", "を", "見た", "手紙", "を", "見つけた"]);

        let tokens: Vec<_> = tokenize("にわのコスモスが、きれいにさきました。", "ja")
            .into_iter()
            .filter(|t| t.w)
            .map(|t| t.t)
            .collect();
        assert_eq!(tokens, ["にわ", "の", "コスモス", "が", "きれいに", "さきました"]);
    }

    #[test]
    fn japanese_offsets_stay_utf16() {
        let tokens = tokenize("日本語 😀 を読む", "ja");
        let last = tokens.iter().find(|token| token.t == "読む").unwrap();
        assert_eq!(last.s, "日本語 😀 を".encode_utf16().count());
        assert_eq!(last.e - last.s, "読む".encode_utf16().count());
    }
}
