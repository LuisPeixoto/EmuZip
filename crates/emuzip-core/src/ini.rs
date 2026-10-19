//! Leitura/escrita de arquivos `chave = valor` usados pelos AVDs.
use std::collections::BTreeMap;
use std::path::Path;

pub type Ini = BTreeMap<String, String>;

pub fn parse(text: &str) -> Ini {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

pub fn read(path: &Path) -> Option<Ini> {
    std::fs::read_to_string(path).ok().map(|t| parse(&t))
}

pub fn write(path: &Path, ini: &Ini) -> std::io::Result<()> {
    let mut s = String::new();
    for (k, v) in ini {
        s.push_str(k);
        s.push('=');
        s.push_str(v);
        s.push('\n');
    }
    std::fs::write(path, s)
}

#[cfg(test)]
mod tests {
    #[test]
    fn roundtrip() {
        let i = super::parse("a = 1\n# c\nb=x y\n");
        assert_eq!(i["a"], "1");
        assert_eq!(i["b"], "x y");
        assert_eq!(i.len(), 2);
    }
}
