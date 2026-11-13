//! Retail replay camera key curve (b404, sampler 82C89A50; tangent 82C89828). Pure math.
//! Formulas: docs/hails-additions/37-instant-replay.md#replay-key-curve

/// One key of one element: where (a clip fraction), the value arriving and leaving (equal for a soft key), whether
/// the element's CUBIC flag is set there, and its TANGENT scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub at: f32,
    pub value_in: f32,
    pub value_out: f32,
    pub cubic: bool,
    pub tangent: f32,
}

impl Key {
    /// A soft key (one value both sides), linear, tangent 1.
    pub fn soft(at: f32, value: f32) -> Self {
        Self { at, value_in: value, value_out: value, cubic: false, tangent: 1.0 }
    }

    pub fn hard(&self) -> bool {
        self.value_in != self.value_out
    }
}

/// Retail's tangent guard (0x8219B100).
pub const EPSILON: f32 = 1e-5;
/// Retail's TANGENT element range (b376).
pub const TANGENT_RANGE: (f32, f32) = (0.0, 8.0);

/// The value at clip fraction `t` of keys sorted by `at` (None without keys: retail holds the live value). Before the
/// first key the first key's in value holds, after the last the last key's out value (the implicit ends).
pub fn sample(keys: &[Key], t: f32, stepped: bool) -> Option<f32> {
    let first = keys.first()?;
    if t <= first.at || keys.len() == 1 {
        return Some(if t <= first.at { first.value_in } else { first.value_out });
    }
    let last = keys.last()?;
    if t >= last.at {
        return Some(last.value_out);
    }
    let k = keys.partition_point(|key| key.at <= t) - 1;
    let (a, b) = (&keys[k], &keys[k + 1]);
    let (p0, p1) = (a.value_out, b.value_in);
    if stepped {
        return Some(p0);
    }
    let span = (b.at - a.at).max(EPSILON);
    let u = (t - a.at) / span;
    let d = p1 - p0;
    // Out tangent at a, in tangent at b, each in the segment's unit parameter.
    let m0 = tangent(keys, k, d, span, false);
    let m1 = tangent(keys, k + 1, d, span, true);
    let ca = m0 + m1 - 2.0 * d;
    let cb = 3.0 * d - 2.0 * m0 - m1;
    Some(((ca * u + cb) * u + m0) * u + p0)
}

/// The tangent at key `i` for the segment of length `dc` and step `d`: linear unless the key is cubic; cubic across
/// a soft key blends in the neighbour segment (before the key when `incoming`, after it otherwise), Bessel style.
fn tangent(keys: &[Key], i: usize, d: f32, dc: f32, incoming: bool) -> f32 {
    let key = &keys[i];
    if !key.cubic {
        return d;
    }
    if key.hard() {
        return d * key.tangent.clamp(TANGENT_RANGE.0, TANGENT_RANGE.1);
    }
    let neighbour = if incoming { keys.get(i + 1).map(|n| (n.value_in - key.value_out, n.at - key.at)) } else { i.checked_sub(1).map(|p| (key.value_in - keys[p].value_out, key.at - keys[p].at)) };
    let m = match neighbour {
        Some((dn_value, dn)) if dn > EPSILON => {
            // The neighbour's step scaled to this segment's parameter length.
            let dn_scaled = dn_value * dc / dn;
            d * dn / (dn + dc) + dn_scaled * dc / (dn + dc)
        }
        _ => d,
    };
    m * key.tangent.clamp(TANGENT_RANGE.0, TANGENT_RANGE.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_by_default_cubic_across_soft_keys_and_cuts_at_hard_keys() {
        // No keys: hold the live value (None). One key: its value everywhere.
        assert_eq!(sample(&[], 0.5, false), None);
        assert_eq!(sample(&[Key::soft(0.5, 3.0)], 0.9, false), Some(3.0));
        // Linear between two soft keys.
        let keys = [Key::soft(0.0, 0.0), Key::soft(1.0, 10.0)];
        assert!((sample(&keys, 0.25, false).unwrap() - 2.5).abs() < 1e-5);
        // Stepped: the segment's start value.
        assert_eq!(sample(&keys, 0.75, true), Some(0.0));
        // Cubic through three evenly spaced keys on a line stays on the line (Catmull-Rom reproduces lines).
        let keys = [0.0, 0.5, 1.0].map(|at| Key { cubic: true, ..Key::soft(at, at * 10.0) });
        assert!((sample(&keys, 0.25, false).unwrap() - 2.5).abs() < 1e-4);
        // Cubic eases through a peak: past the middle key's value nowhere, smooth either side.
        let keys = [Key::soft(0.0, 0.0), Key { cubic: true, ..Key::soft(0.5, 10.0) }, Key::soft(1.0, 0.0)];
        let v = sample(&keys, 0.45, false).unwrap();
        assert!(v > 9.0 && v < 10.0, "{v}");
        // A hard key cuts: the value jumps at the key.
        let keys = [Key::soft(0.0, 0.0), Key { value_out: 50.0, ..Key::soft(0.5, 5.0) }, Key::soft(1.0, 60.0)];
        assert!((sample(&keys, 0.4999, false).unwrap() - 5.0).abs() < 0.01);
        assert!((sample(&keys, 0.5001, false).unwrap() - 50.0).abs() < 0.01);
    }
}
