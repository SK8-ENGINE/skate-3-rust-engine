"""Verify installed runtime dependencies before publishing setup completion."""
import hashlib
import json
from pathlib import Path

from tools.install_prepared_hud import runtime_files
from .original_content import destination


def validate(stage, maps, expected_backdrops=None):
    assets = stage / "assets"
    files = {}

    def require(relative):
        path = destination(assets, relative)
        if relative in files:
            return path
        if not path.is_file() or not path.stat().st_size:
            raise ValueError(f"Setup is missing runtime content: {relative}")
        with path.open("rb") as stream:
            sha = hashlib.file_digest(stream, "sha256").hexdigest()
        files[relative] = sha
        return path

    def read(relative):
        return json.loads(require(relative).read_text(encoding="utf-8"))

    manifest = read("private/game.json")
    for key in ("character_scene", "action_graph", "motion_graph"):
        require(manifest[key])
    for name in ("OnBoard", "OffBoard"):
        require(f"private/stock/data/anim/{name}.abin")
    for name in ("skater-collections.json", "physics-skeletons.json", "data/config/input.cfg",
                 "data/script/camera/Default_cameragraph.stategraph", "data/camera/1.shk", "data/camera/2.shk"):
        require("private/stock/" + name)
    for folder, marker in (("hud", False), ("session-marker", True)):
        for relative in runtime_files(assets / "private" / folder, marker):
            require("private/" + folder + "/" + relative)
    library = read("private/customisation/library-v3.json")
    if library.get("version") != 3 or library.get("errors") or not library.get("models"):
        raise ValueError("Setup character library is incomplete")
    read("private/customisation/extra-menu.json")
    for model in library["models"].values():
        require(model["scene"])
    for material in library["materials"].values():
        for key in ("diffuse", "normal", "rough", "opacity"):
            if material.get(key):
                require(material[key])
    for tattoo in library["tattoos"].values():
        require(tattoo["texture"])
    lighting = read("private/character-lighting.json")
    for material in lighting["materials"].values():
        if material.get("specular"):
            require(material["specular"])
    for name in ("render-parameters", "exposure", "exposure-profiles", "teleports"):
        read(f"private/{name}.json")
    backdrops = list((assets / "private/native-backdrops").glob("*.skate"))
    if expected_backdrops is not None and len(backdrops) != expected_backdrops:
        raise ValueError("Setup foliage backdrop count does not match conversion")
    for backdrop in backdrops:
        require(backdrop.relative_to(assets).as_posix())
    if not maps:
        raise ValueError("Setup has no converted maps")
    for row in maps:
        path = destination(stage, row["path"])
        if not path.is_file() or not path.stat().st_size:
            raise ValueError(f"Setup map is missing: {row['path']}")
        name = row["name"]
        sky = read(f"private/native-skies/{name}.json")
        for suffix, width, height in (("rgba", sky["width"], sky["height"]),
                                      ("sun.rgba", sky["sun_width"], sky["sun_height"])):
            if require(f"private/native-skies/{name}.{suffix}").stat().st_size != width * height * 4:
                raise ValueError(f"Invalid sky texture: {name}.{suffix}")
        props = read(f"private/native-props/{name}.json")
        if props.get("instances"):
            require(f"private/native-props/{name}.skate")
    original = read("private/original/manifest.json")
    if original.get("version") != 1 or not original.get("files"):
        raise ValueError("Setup has no indexed original content")
    result = dict(version=1, runtime_files=files, maps=maps,
                  original_files=len(original["files"]),
                  preserved_source_only=["Unported audio and frontend payloads",
                                         "Executable-resident tables without a disc decoder (ocean PCA)"])
    temporary = assets / "private/content-manifest.json.new"
    temporary.write_text(json.dumps(result, indent=2), encoding="utf-8")
    temporary.replace(assets / "private/content-manifest.json")
    return result
