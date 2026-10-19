//! Traduções: mesmas `.po` usadas pelo Slint (`translations/<lang>/LC_MESSAGES/emuzip-app.po`).
use std::collections::HashMap;
use std::sync::RwLock;

/// (código salvo nas configurações, pasta do .po, nome exibido)
pub const LANGUAGES: [(&str, &str, &str); 7] = [
    ("system", "", "System"),
    ("en", "", "English"),
    ("pt-BR", "pt_BR", "Português (Brasil)"),
    ("es", "es", "Español"),
    ("fr", "fr", "Français"),
    ("de", "de", "Deutsch"),
    ("zh-CN", "zh_CN", "中文 (简体)"),
];

const CATALOGS: [(&str, &str); 5] = [
    ("pt_BR", include_str!("../translations/pt_BR/LC_MESSAGES/emuzip-app.po")),
    ("es", include_str!("../translations/es/LC_MESSAGES/emuzip-app.po")),
    ("fr", include_str!("../translations/fr/LC_MESSAGES/emuzip-app.po")),
    ("de", include_str!("../translations/de/LC_MESSAGES/emuzip-app.po")),
    ("zh_CN", include_str!("../translations/zh_CN/LC_MESSAGES/emuzip-app.po")),
];

static CURRENT: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);

fn unquote(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s);
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parser mínimo de .po (msgid/msgstr com linhas de continuação; msgctxt vira prefixo `ctx\u{4}`; ignora plurais).
pub fn parse_po(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let (mut ctx, mut id, mut st) = (String::new(), String::new(), String::new());
    // Some(0) = msgctxt, Some(1) = msgid, Some(2) = msgstr
    let mut cur: Option<u8> = None;
    let mut flush = |ctx: &mut String, id: &mut String, st: &mut String| {
        if !id.is_empty() && !st.is_empty() {
            let key = if ctx.is_empty() { std::mem::take(id) } else { format!("{ctx}\u{4}{id}") };
            map.insert(key, std::mem::take(st));
        }
        ctx.clear();
        id.clear();
        st.clear();
    };
    for line in text.lines() {
        let l = line.trim();
        if let Some(r) = l.strip_prefix("msgctxt ") {
            flush(&mut ctx, &mut id, &mut st);
            ctx = unquote(r);
            cur = Some(0);
        } else if let Some(r) = l.strip_prefix("msgid ") {
            if cur != Some(0) {
                flush(&mut ctx, &mut id, &mut st);
            }
            id = unquote(r);
            cur = Some(1);
        } else if let Some(r) = l.strip_prefix("msgstr ") {
            st = unquote(r);
            cur = Some(2);
        } else if l.starts_with('"') {
            match cur {
                Some(0) => ctx.push_str(&unquote(l)),
                Some(1) => id.push_str(&unquote(l)),
                Some(2) => st.push_str(&unquote(l)),
                _ => {}
            }
        } else if l.is_empty() {
            cur = None;
        }
    }
    flush(&mut ctx, &mut id, &mut st);
    map
}

/// Resolve "system" para um idioma suportado.
pub fn resolve(code: &str) -> &'static str {
    let code = if code == "system" || code.is_empty() { sys_locale::get_locale().unwrap_or_default() } else { code.to_string() };
    let base = code.split(['-', '_']).next().unwrap_or("").to_lowercase();
    match base.as_str() {
        "pt" => "pt-BR",
        "es" => "es",
        "fr" => "fr",
        "de" => "de",
        "zh" => "zh-CN",
        _ => "en",
    }
}

/// Aplica o idioma no Rust e no Slint (troca ao vivo).
pub fn set_language(code: &str) {
    let lang = resolve(code);
    let folder = LANGUAGES.iter().find(|l| l.0 == lang).map(|l| l.1).unwrap_or("");
    *CURRENT.write().unwrap() = CATALOGS.iter().find(|c| c.0 == folder).map(|c| parse_po(c.1));
    let _ = slint::select_bundled_translation(folder);
}

pub fn t(s: &str) -> String {
    CURRENT.read().unwrap().as_ref().and_then(|m| m.get(s).cloned()).unwrap_or_else(|| s.to_string())
}

/// Como `t`, desambiguando por contexto (`msgctxt`).
pub fn t_ctx(ctx: &str, s: &str) -> String {
    CURRENT.read().unwrap().as_ref().and_then(|m| m.get(&format!("{ctx}\u{4}{s}")).cloned()).unwrap_or_else(|| s.to_string())
}

/// Substitui cada `{}` pelos argumentos, na ordem.
pub fn fmt(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut it = args.iter();
    let mut rest = template;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        match it.next() {
            Some(a) => out.push_str(&a.to_string()),
            None => out.push_str("{}"),
        }
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// `tr!("Text")`, `tr!("Starting {}", name)` ou `tr!("ctx" => "Text", ...)`: traduz e formata.
#[macro_export]
macro_rules! tr {
    ($c:literal => $s:literal) => { $crate::i18n::t_ctx($c, $s) };
    ($c:literal => $s:literal, $($a:expr),+ $(,)?) => { $crate::i18n::fmt(&$crate::i18n::t_ctx($c, $s), &[$(&$a as &dyn std::fmt::Display),+]) };
    ($s:literal) => { $crate::i18n::t($s) };
    ($s:literal, $($a:expr),+ $(,)?) => { $crate::i18n::fmt(&$crate::i18n::t($s), &[$(&$a as &dyn std::fmt::Display),+]) };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn po() {
        let m = parse_po("msgid \"\"\nmsgstr \"Content-Type: text/plain\"\n\nmsgid \"Hello {}\"\nmsgstr \"Olá {}\"\n\nmsgid \"A\"\n\"B\"\nmsgstr \"\"\n\"C\\nD\"\n\nmsgid \"Empty\"\nmsgstr \"\"\n\nmsgctxt \"f\"\nmsgid \"None\"\nmsgstr \"Nenhuma\"\n\nmsgid \"None\"\nmsgstr \"Nenhum\"\n");
        assert_eq!(m["Hello {}"], "Olá {}");
        assert_eq!(m["AB"], "C\nD");
        assert!(!m.contains_key("Empty"));
        assert_eq!(m["f\u{4}None"], "Nenhuma");
        assert_eq!(m["None"], "Nenhum");
        assert_eq!(fmt("{} de {}", &[&1, &"x"]), "1 de x");
        assert_eq!(fmt("sem args {}", &[]), "sem args {}");
    }
}
