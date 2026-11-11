//! Cross-movie imports for the HUD movies (doc 42, "Tutorial callout"): a HUD movie that imports menu library movies
//! (hudtutorial: source/controls/gesture_item, button_item2) links them from the exported menu movie set, the way the
//! menu stage does (apt_movie `link_imports`). Imported shapes are copied into the HUD's shape map with their texture
//! paths made relative to the HUD folder.
use crate::apt_imports::{library_key, resolve, MovieDecl};
use crate::apt_movie::Movie;
use crate::apt_scene::Shapes;
use std::collections::BTreeMap;
use std::path::Path;

/// Where the menu movie set sits next to `private/hud` (texture paths in it are relative to that folder).
const MENU_FROM_HUD: &str = "../menu-movies";

/// A HUD movie's import table. tools/prepare_hud.py writes its `exports` as bare character ids (no linkage names), so
/// they are left out: nothing imports from a HUD movie.
fn hud_decl(json: &serde_json::Value) -> Result<MovieDecl, String> {
    let mut decl = json.clone();
    if let Some(exports) = decl.get_mut("exports").and_then(|e| e.as_array_mut()) {
        exports.retain(|e| e.is_object());
    }
    MovieDecl::from_json(&decl)
}

#[derive(Clone, Default)]
pub struct HudLibraries {
    decls: BTreeMap<String, MovieDecl>,
    movies: BTreeMap<String, Movie>,
    shapes: BTreeMap<String, Shapes>,
    /// Libraries that did not load, with the reason.
    pub failures: Vec<String>,
    /// Each library's root init actions (class definitions, registerClass), libraries a library imports first: retail
    /// loads every imported library before linking (sub_82E7E8C0) and they run then.
    pub init: Vec<Vec<crate::apt_vm::Instruction>>,
}

impl HudLibraries {
    /// Loads every library the given HUD movies import, transitively, from `<menu_root>/movies/<key>.json`. A library
    /// that is missing or does not load stays absent (its import is left empty, like retail's null slot).
    pub fn load(menu_root: &Path, sources: &[&serde_json::Value]) -> Self {
        let mut libs = Self::default();
        // The language table, fonts and native font data every menu movie shares (retail_menu_movies `merged`).
        let shared = std::fs::read(menu_root.join("shared.json")).ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        let mut queue: Vec<String> = sources.iter().filter_map(|s| hud_decl(s).ok()).flat_map(|d| d.imports.into_iter().map(|i| library_key(&i.file))).collect();
        while let Some(key) = queue.pop() {
            if libs.decls.contains_key(&key) || libs.decls.len() >= crate::apt_imports::MAX_MOVIES {
                continue;
            }
            let loaded = (|| -> Result<_, String> {
                let path = menu_root.join("movies").join(format!("{key}.json"));
                let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                let json = crate::retail_menu_movies::merged(serde_json::from_slice(&bytes).map_err(|e| e.to_string())?, &shared);
                let shapes = serde_json::from_value::<Shapes>(json["shapes"].clone()).map_err(|e| format!("shapes: {e}"))?;
                Ok((MovieDecl::from_json(&json)?, Movie::load(&json)?, shapes))
            })();
            let (decl, movie, shapes) = match loaded {
                Ok(l) => l,
                Err(e) => {
                    bevy::log::warn!("HUD import library {key} does not load: {e}");
                    libs.failures.push(format!("{key}: {e}"));
                    continue;
                }
            };
            queue.extend(decl.imports.iter().map(|i| library_key(&i.file)));
            let root_init: Vec<_> = movie.characters.get(&0).into_iter().flat_map(|c| &c.frames).flat_map(|f| &f.controls)
                .filter(|c| c.type_name == "do_init_action").filter_map(|c| movie.actions.get(&c.actions_offset.to_string()).cloned()).collect();
            libs.init.splice(0..0, root_init);
            libs.decls.insert(key.clone(), decl);
            libs.movies.insert(key.clone(), movie);
            libs.shapes.insert(key, shapes);
        }
        libs
    }

    pub fn is_empty(&self) -> bool {
        self.movies.is_empty()
    }

    /// The HUD movie with its imports linked.
    pub fn link(&self, key: &str, json: &serde_json::Value) -> Result<Movie, String> {
        let mut movie = Movie::load(json)?;
        if self.is_empty() {
            return Ok(movie);
        }
        let decl = hud_decl(json)?;
        let resolution = resolve(key, decl, &mut self.decls.clone());
        movie.link_imports(key, &resolution, &self.movies)?;
        Ok(movie)
    }

    /// The shapes `movie` copied in, keyed by their ids in it, textures re-rooted at the HUD folder.
    pub fn imported_shapes(&self, movie: &Movie) -> Shapes {
        let mut out = Shapes::new();
        for (local, (key, id)) in &movie.shape_origins {
            let Some(shapes) = self.shapes.get(key).and_then(|s| s.get(id)) else { continue };
            let mut shapes = shapes.clone();
            for t in shapes.iter_mut().filter_map(|s| s.texture.as_mut()) {
                t.rgba = format!("{MENU_FROM_HUD}/{}", t.rgba);
            }
            out.insert(*local, shapes);
        }
        out
    }
}
