use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=locales");
    println!("cargo:rerun-if-changed=src/i18n.rs");
    let macros = fs::read_to_string("src/i18n.rs").expect("Read localized format macros");
    let languages = [("en", "English"), ("ko", "Korean"), ("ja", "Japanese"), ("fr", "French"), ("de", "German"), ("ru", "Russian")];
    let catalogs: Vec<BTreeMap<String, String>> = languages.iter().map(|(code, _)| {
        serde_json::from_str(&fs::read_to_string(format!("locales/{code}.json")).expect("Read language catalog")).expect("Parse language catalog")
    }).collect();
    let mut output = String::from("fn translate(text: &str, language: Language) -> &str { match language {\n");
    for ((_, name), catalog) in languages.iter().zip(&catalogs) {
        assert!(catalog.keys().eq(catalogs[0].keys()), "Every language must cover the English catalog");
        for (key, value) in catalog.iter().filter(|(key, _)| key.contains('{')) {
            assert!(macros.contains(format!("{value:?}").as_str()), "Stale format translation for {key}; run node scripts/sync-i18n.mjs then cargo fmt");
        }
        output.push_str(&format!("Language::{name} => match text {{\n"));
        for (key, value) in catalog { output.push_str(&format!("{key:?} => {value:?},\n")); }
        output.push_str("_ => text, },\n");
    }
    output.push_str("_ => text, } }\n");
    // Only these placeholders are language-dependent. Keep user-entered values intact.
    output.push_str("fn placeholder_source(text: &str) -> &str { match text {\n");
    for key in ["例如：生产环境 / 个人存储", "留空保留现有凭据", "你的 Secret Access Key", "使用临时凭据时填写；更新凭据时留空可清除"] {
        output.push_str(&format!("{key:?}"));
        for catalog in &catalogs { output.push_str(&format!(" | {:?}", catalog[key])); }
        output.push_str(&format!(" => {key:?},\n"));
    }
    output.push_str("_ => text, } }\n");
    fs::write(PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("translations.rs"), output).expect("Write translations");
}
