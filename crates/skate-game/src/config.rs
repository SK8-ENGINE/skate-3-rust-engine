use bevy::prelude::Resource;
use std::path::PathBuf;

#[derive(Resource)]
pub(crate) struct Config {
    pub asset_root: PathBuf,
    pub verification_capture: Option<PathBuf>,
    pub map: Option<skate_data::skate_map::SkateMap>,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let mut config = Self {
            asset_root: PathBuf::from("assets"),
            verification_capture: None,
            map: None,
        };
        let mut args = std::env::args_os().skip(1);
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--assets") => {
                    config.asset_root = args.next().ok_or("--assets requires a directory")?.into()
                }
                Some("--map") => {
                    let path = PathBuf::from(args.next().ok_or("--map requires a .skate file")?);
                    config.map = Some(skate_data::skate_map::SkateMap::load(&path)?);
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
                        "Unknown argument {arg:?}. Usage: skate-game [--assets DIRECTORY] [--map MAP.skate] [--verify CAPTURE.png]"
                    ));
                }
            }
        }
        config.asset_root = config
            .asset_root
            .canonicalize()
            .map_err(|e| format!("Asset root {}: {e}", config.asset_root.display()))?;
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
