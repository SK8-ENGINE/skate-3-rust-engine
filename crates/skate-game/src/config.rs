use bevy::prelude::Resource;
use std::path::PathBuf;

#[derive(Resource)]
pub(crate) struct Config {
    pub asset_root: PathBuf,
    pub verification_capture: Option<PathBuf>,
    pub map: Option<skate_data::skate_map::SkateMap>,
    pub map_path: Option<PathBuf>,
    pub difficulty: crate::difficulty::Difficulty,
    pub check_assets: bool,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let mut config = Self {
            asset_root: crate::setup::asset_root()?,
            verification_capture: None,
            map: None,
            map_path: None,
            difficulty: crate::difficulty::Difficulty::default(),
            check_assets: false,
        };
        let mut difficulty_override = None;
        let mut explicit_map = false;
        let mut args = std::env::args_os().skip(1);
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--assets") => {
                    config.asset_root = args.next().ok_or("--assets requires a directory")?.into()
                }
                Some("--map") => {
                    let path = PathBuf::from(args.next().ok_or("--map requires a .skate file")?);
                    config.map = Some(skate_data::skate_map::SkateMap::load(&path)?);
                    config.map_path = Some(path.canonicalize().map_err(|e| e.to_string())?);
                    explicit_map = true;
                }
                Some("--test-world") => { explicit_map = true; config.map = None; config.map_path = None; }
                Some("--check-assets") => config.check_assets = true,
                Some("--difficulty") => {
                    let value = args.next().ok_or("--difficulty requires easy, normal or hardcore")?;
                    difficulty_override = Some(crate::difficulty::Difficulty::parse(&value.to_string_lossy())?);
                }
                Some("--verify") => {
                    config.verification_capture = Some(
                        args.next()
                            .ok_or("--verify requires an output PNG path")?
                            .into(),
                    )
                }
                _ => {
                    return Err(format!(
                        "Unknown argument {arg:?}. Usage: skate3rust [--assets DIRECTORY] [--map MAP.skate | --test-world] [--difficulty easy|normal|hardcore] [--verify CAPTURE.png] [--check-assets]"
                    ));
                }
            }
        }
        config.asset_root = config
            .asset_root
            .canonicalize()
            .map_err(|e| format!("Asset root {}: {e}", config.asset_root.display()))?;
        config.difficulty = match difficulty_override {
            Some(mode) => mode,
            None => crate::difficulty::Difficulty::load(&config.asset_root)?,
        };
        if !explicit_map {
            if let Some(path) = crate::map_library::default_map(&config.asset_root)? {
                config.map = Some(skate_data::skate_map::SkateMap::load(&path)?);
                config.map_path = Some(path.canonicalize().map_err(|e| e.to_string())?);
            }
        }
        if let Some(path) = &mut config.verification_capture {
            if path.extension().and_then(|x| x.to_str()) != Some("png") {
                return Err("--verify output must be a PNG file".into());
            }
            if !path.is_absolute() {
                *path = std::env::current_dir()
                    .map_err(|e| e.to_string())?
                    .join(&path);
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        Ok(config)
    }
}
