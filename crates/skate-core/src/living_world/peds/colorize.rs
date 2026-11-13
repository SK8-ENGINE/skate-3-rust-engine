//! Ped clothing colours: the ped shaders recolour mask texels with the model's tint pair; baked per pair here.
//! Shader rule and data: docs/hails-additions/26f-living-world-peds.md#ped-colorize

/// The mask test literal of both ped pixel shaders: `G^2 < 0.001225` [code, shader].
pub const MASK_GREEN_SQ_MAX: f32 = 0.001_225;

/// Which material types the recolour applies to: retail's ped body materials
/// (`pedestrian_high_stamp`, `pedestrian_low`) [data, recipe XML material types]. A mod GLB
/// opts in by tagging its material with one of these.
pub fn colorized_shader(shader: &str) -> bool {
    matches!(shader, "pedestrian_high_stamp" | "pedestrian_low")
}

/// One texel (sRGB-encoded values in 0..=1): the shader rule, returned in the same encoding.
pub fn colorize_texel(rgb: [f32; 3], red: [f32; 4], blue: [f32; 4]) -> [f32; 3] {
    let [r, g, b] = rgb.map(|c| c * c);
    if g >= MASK_GREEN_SQ_MAX {
        return rgb;
    }
    [0, 1, 2].map(|i| (r * red[i] + b * blue[i]).max(0.0).sqrt().min(1.0))
}

/// Recolour an RGBA8 (sRGB) pixel buffer in place; alpha is kept.
pub fn colorize_rgba8(pixels: &mut [u8], red: [f32; 4], blue: [f32; 4]) {
    for px in pixels.chunks_exact_mut(4) {
        let out = colorize_texel([px[0], px[1], px[2]].map(|c| c as f32 / 255.0), red, blue);
        for i in 0..3 {
            px[i] = (out[i] * 255.0).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE_RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const BASE_BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

    #[test]
    fn base_palette_is_the_identity_on_mask_texels() {
        for rgb in [[0.8, 0.0, 0.0], [0.0, 0.02, 0.6], [0.5, 0.03, 0.25], [0.0, 0.0, 0.0]] {
            let out = colorize_texel(rgb, BASE_RED, BASE_BLUE);
            for i in [0, 2] {
                assert!((out[i] - rgb[i]).abs() < 1e-6, "{rgb:?} -> {out:?}");
            }
            assert_eq!(out[1], 0.0);
        }
    }

    #[test]
    fn mask_texels_take_the_tints_and_others_keep_their_colour() {
        let shirt = [0.9, 0.5, 0.2, 1.0];
        let jeans = [0.1, 0.2, 0.4, 1.0];
        // pure red mask at full strength -> the red tint (lin = 1 x shirt)
        let out = colorize_texel([1.0, 0.0, 0.0], shirt, jeans);
        for i in 0..3 {
            assert!((out[i] - shirt[i].sqrt()).abs() < 1e-6);
        }
        // pure blue mask at half strength -> 0.25 x jeans in squared space
        let out = colorize_texel([0.0, 0.0, 0.5], shirt, jeans);
        for i in 0..3 {
            assert!((out[i] - (0.25 * jeans[i]).sqrt()).abs() < 1e-6);
        }
        // skin: green well above the threshold -> unchanged
        assert_eq!(colorize_texel([0.8, 0.6, 0.5], shirt, jeans), [0.8, 0.6, 0.5]);
        // the threshold: green 0.0351 is not a mask texel, 0.0349 is
        assert_eq!(colorize_texel([0.5, 0.0351, 0.5], shirt, jeans), [0.5, 0.0351, 0.5]);
        assert_ne!(colorize_texel([0.5, 0.0349, 0.5], shirt, jeans), [0.5, 0.0349, 0.5]);
    }

    #[test]
    fn rgba8_keeps_alpha_and_is_deterministic() {
        let mut a = vec![255, 0, 0, 77, 200, 180, 160, 255];
        colorize_rgba8(&mut a, [0.25, 0.25, 0.25, 1.0], BASE_BLUE);
        assert_eq!(a, vec![128, 128, 128, 77, 200, 180, 160, 255]);
        assert!(colorized_shader("pedestrian_high_stamp") && colorized_shader("pedestrian_low"));
        assert!(!colorized_shader("marquee_hair") && !colorized_shader("marquee_cloth"));
    }
}
