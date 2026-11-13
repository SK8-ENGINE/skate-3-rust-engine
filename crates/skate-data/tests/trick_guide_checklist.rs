//! Writes the Trick Guide demo checklist (row, clip, clip exists). Env: `SKATE3_ASSETS`, `SKATE3_MENU_LANGUAGE`,
//! `SKATE3_SCENE_CLIPS`, output `SKATE3_CHECKLIST_OUT`.
use skate_data::{collections::Collections, menu_tables::MenuTables};
use std::{collections::BTreeMap, fmt::Write, path::PathBuf};

#[test]
#[ignore = "needs owned assets; writes a review checklist"]
fn write_trick_guide_clip_checklist() {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let assets = env("SKATE3_ASSETS").unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
    let (Some(language), Some(clips), Some(out)) = (env("SKATE3_MENU_LANGUAGE"), env("SKATE3_SCENE_CLIPS"), env("SKATE3_CHECKLIST_OUT")) else {
        eprintln!("skipped: set SKATE3_MENU_LANGUAGE, SKATE3_SCENE_CLIPS and SKATE3_CHECKLIST_OUT");
        return;
    };
    let collections = Collections::load(&assets).unwrap();
    let tables = MenuTables::from_json(&std::fs::read_to_string(assets.join("private/menu-tables.json")).unwrap()).unwrap();
    let guide = skate_data::trick_guide::load(&collections, &tables.trick_guide);
    let shared: serde_json::Value = serde_json::from_slice(&std::fs::read(language).unwrap()).unwrap();
    let text: BTreeMap<String, String> = serde_json::from_value(shared["language"].clone()).unwrap();
    let name = |id: &str| text.get(id).cloned().unwrap_or_else(|| id.to_owned());
    let clip = |animation: &str| {
        if animation.is_empty() {
            "(keeps the previous clip)".to_owned()
        } else {
            let file = format!("{animation}1.abin");
            let found = if clips.join(&file).is_file() { "" } else { " **MISSING**" };
            format!("`{file}`{found}")
        }
    };
    let (mut md, mut rows, mut missing) = (String::new(), 0, 0);
    writeln!(md, "# Trick Guide demo clips: review checklist\n").unwrap();
    writeln!(md, "Generated from the game data (`crates/skate-data/tests/trick_guide_checklist.rs`). Each row plays the clip \
        shown while it is highlighted (after a 60 ms settle). Tick a row once it looks right in game.\n").unwrap();
    for key in &guide.categories {
        let Some(category) = guide.category.get(key) else { continue };
        writeln!(md, "## {}\n", name(&category.entry.label)).unwrap();
        for sub_key in &category.subcategories {
            let Some(sub) = guide.subcategory.get(sub_key) else { continue };
            writeln!(md, "### {}  (row clip: {})\n", name(&sub.entry.label), clip(&sub.entry.animation)).unwrap();
            let mut tricks: Vec<_> = guide.tricks.iter()
                .filter(|(_, t)| !t.entry.animation.is_empty() && t.subcategory == Some(*sub_key)).collect();
            tricks.sort_by_key(|(_, t)| t.order);
            if tricks.is_empty() {
                writeln!(md, "(no trick list)\n").unwrap();
                continue;
            }
            writeln!(md, "| OK | Trick | Clip |\n|---|---|---|").unwrap();
            for (_, t) in tricks {
                let c = clip(&t.entry.animation);
                missing += c.contains("MISSING") as usize;
                rows += 1;
                writeln!(md, "| [ ] | {} | {c} |", name(&t.entry.label)).unwrap();
            }
            writeln!(md).unwrap();
        }
    }
    writeln!(md, "---\n{rows} trick rows, {missing} without a clip file.").unwrap();
    std::fs::write(&out, md).unwrap();
    eprintln!("CHECKLIST rows={rows} missing={missing} -> {}", out.display());
}
