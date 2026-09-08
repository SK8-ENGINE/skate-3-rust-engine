"""Incrementally extract the owned session-marker HUD into a private overlay.

Uses the existing UI toolkit's bounds-checked APT/GEO/RX2/font readers. No game
process, captures, font substitution or redrawn artwork is involved.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import sys


def compile_hud(cache_root: Path, output: Path) -> None:
    from skate3_ui_extract.scene_graph import AssetCache, SceneFlattener, transform_point
    from skate3_ui_extract.bitmap_font import measure_bitmap_text
    from skate3_ui_extract.timeline import playback_frame
    cache = AssetCache(cache_root)
    bundle = "data/fe/source/screens/hud2/hudphonelist"
    labels = json.loads((cache_root / "metadata/languages/english_global.json").read_text())
    language = {e["label"]: e["value"] for e in labels["entries"] if "label" in e}

    def state(path, owner, character):
        if path == "/mSkatePark": return {"visible": False}
        if character["type_name"] in ("sprite", "animation"):
            count = character["movie"]["frame_count"]
            if owner["key"] == bundle:
                label = {0: "hudintro", 17: "maximized", 16: "2"}.get(character["id"])
                if label: return {"frame": playback_frame(character, label, play=True)}
            if count <= 1: return {"frame": 0}
        return {}

    flat = SceneFlattener(cache, state).flatten(bundle, 0)
    if flat["unresolved"]: raise ValueError(flat["unresolved"])
    output.mkdir(parents=True, exist_ok=True)
    textures = []
    texture_ids = {}

    def texture(path, width=None, height=None):
        path = str(path)
        if path in texture_ids: return texture_ids[path]
        from PIL import Image
        src = cache_root / path
        with Image.open(src) as im:
            im = im.convert("RGBA")
            width, height = im.size
            index = len(textures)
            name = f"texture-{index}.rgba"
            (output / name).write_bytes(im.tobytes())
        texture_ids[path] = index
        textures.append({"file": name, "width": width, "height": height,
            "source": path, "sha256": hashlib.sha256(src.read_bytes()).hexdigest()})
        return index

    buttons_dir = Path("assets/data/fe/source/images/buttons/xbox360/buttons")
    buttons = json.loads((cache_root / buttons_dir / "manifest.json").read_text())["textures"]
    meshes = []
    for primitive in flat["primitives"]:
        tex = primitive.get("texture")
        index = texture(tex["preview"]) if tex else None
        role = "art"
        if "mButtonRender" in primitive["path"]:
            role = "return" if "/mButton0/" in primitive["path"] else "place"
            name = "button_DPad_Up_hud.Texture" if role == "return" else "button_DPad_Down_hud.Texture"
            item = next(t for t in buttons if t["name"] == name)
            index = texture(buttons_dir / item["preview_file"])
            # SetButtonType replaces the authored 32px button render rectangle.
            # Preserve its original geometry/placement and bind the retail icon.
            inv = primitive["matrix"]
            for triangle in primitive["triangles"]:
                for vertex in triangle:
                    x, y = vertex["position"]
                    vertex["uv"] = [(x - inv[4] + 16.5) / 32, (y - inv[5] + 16.5) / 32]
            primitive["color"] = [1, 1, 1, primitive["color"][3]]
        meshes.append({"texture": index, "role": role, "color": primitive["color"],
            "vertices": [dict(position=v["position"], uv=v.get("uv", [0, 0]))
                         for tri in primitive["triangles"] for v in tri],
            "order": primitive["draw_order"], "source": primitive["path"]})
    for text in flat["text"]:
        value = language.get(text["value"], text["value"])
        if value.startswith("ID_"): raise ValueError(f"Missing retail text {value}")
        font = text["font_asset"]
        definition = font["definition"]
        metrics = measure_bitmap_text(definition, value, text["font_height"])
        index = texture(font["preview"])
        atlas = textures[index]
        glyphs = {g["glyph_index"]: g for g in definition["glyphs"]}
        vertices = []
        scale = metrics["scale"]
        for used in metrics["glyphs"]:
            g = glyphs[used["glyph_index"]]
            x = text["bounds"][0] + 2 + used["left"]
            y = text["bounds"][1] + 2 + text["font_height"] - g["y_offset"] * scale
            w, h = g["width"] * scale, g["height"] * scale
            a, b, c, d = g["atlas_bounds"]
            corners = [(x,y,a,b), (x+w,y,c,b), (x+w,y+h,c,d), (x,y+h,a,d)]
            for corner in (0,1,2,0,2,3):
                px,py,u,v = corners[corner]
                vertices.append({"position": transform_point(text["matrix"], [px,py]),
                    "uv": [u/atlas["width"],v/atlas["height"]]})
        argb = int(text["color_argb"].lstrip("#"), 16)
        meshes.append({"texture": index, "role": "return" if "/text0/" in text["path"] else "place",
            "color": [((argb >> n) & 255)/255 for n in (16,8,0)] + [text["alpha"]*((argb>>24)&255)/255],
            "vertices": vertices, "order": text["draw_order"], "source": text["path"]})
    manifest = {"version": 1, "canvas": [1280,720], "textures": textures,
        "meshes": sorted(meshes, key=lambda m:m["order"]),
        "source_manifest_sha256": hashlib.sha256((cache_root/"manifest.json").read_bytes()).hexdigest(),
        "timelines": {"hudintro": [1,14], "hudoutro": [15,30], "maximized": [27,49]},
        "notes": "Authored display lists; two session-marker rows; dynamic button textures resolved by native SetButtonType names."}
    (output/"hud.json").write_text(json.dumps(manifest, indent=2)+"\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game", type=Path, required=True)
    parser.add_argument("--ui-toolkit", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve().is_relative_to(args.game.resolve()):
        parser.error("Output must be outside the owned game source")
    sys.path.insert(0, str(args.ui_toolkit.resolve()))
    from skate3_ui_extract.project import extract_project
    cache = args.output / "source-cache"
    extract_project(args.game, cache, include_dynamic=True, prefixes=(
        "data/fe/source/screens/hud2/hudphonelist", "data/fe/source/controls/button_item2",
        "data/fe/source/images/buttons/xbox360"), update=True)
    compile_hud(cache, args.output)
    print(f"Private original session-marker HUD: {args.output / 'hud.json'}")


if __name__ == "__main__": main()
