//! Challenge AI path filters (`AI.FilterPaths` 8284A4C8): a row's path names (`tRoundPaths`, race heats `AIPaths`,
//! contest rounds `Paths`) become 16-byte filter ids (`sub_824570F0`) that a line id must match (`sub_82457620`) for a
//! challenge rider to take that line. Details: docs/hails-additions/42-challenges.md#ai-path-filters

/// The name prefixes (table 0x830432FC); the index is id byte 0 (1 = challenge line, 0 = ambient).
pub const PREFIXES: [&str; 6] = ["am", "ch", "ob", "te", "*", "am_*"];
/// The challenge kinds (table 0x830432B0); the index is id byte 5.
pub const KINDS: [&str; 17] =
    ["1up", "cap", "caf", "demo", "dice", "film", "ftl", "goto", "hom", "otl", "ots", "photo", "race", "skate", "domination", "street", "vert"];
/// A kind byte that matches every kind (`sub_82457620`).
pub const ANY_KIND: u8 = 0x11;
/// A prefix byte that matches every challenge line (`sub_82457620`).
pub const ANY_LINE: u8 = 4;

/// `sub_82457928`: the longest table name `s` starts with -> (index, rest after it). None: no name matches.
fn longest(s: &[u8], table: &[&str]) -> Option<(usize, usize)> {
    table.iter().enumerate().filter(|(_, n)| s.starts_with(n.as_bytes())).max_by_key(|(i, n)| (n.len(), usize::MAX - i)).map(|(i, n)| (i, n.len()))
}

/// `sub_824579C8`: leading decimal digits (0 when none); returns (value, index of the first non-digit).
fn number(s: &[u8]) -> (u32, usize) {
    let n = s.iter().take_while(|c| c.is_ascii_digit()).count();
    (s[..n].iter().fold(0u32, |v, c| v.wrapping_mul(10).wrapping_add(u32::from(c - b'0'))), n)
}

/// `sub_82457A20`: the token up to `_` or the end, and the index after its `_`.
fn token(s: &[u8]) -> (&[u8], usize) {
    let n = s.iter().position(|&c| c == b'_').unwrap_or(s.len());
    (&s[..n], if n < s.len() { n + 1 } else { n })
}

/// Whether another token follows the one at the start of `s` (`sub_82457780` / `sub_82457850`).
fn has_next(s: &[u8]) -> bool {
    s.iter().position(|&c| c == b'_').is_some_and(|i| i + 1 < s.len())
}

fn skip(s: &[u8], at: usize) -> &[u8] {
    s.get(at + 1..).unwrap_or(&[])
}

/// `sub_824570F0`: the filter id of a path name such as `ch_race_indu_01_y_0` = prefix, kind, location (4 bytes),
/// instance, subset (only when a further token follows; up to 7 bytes), line index.
pub fn filter_id(name: &str) -> [u8; 16] {
    let mut id = [0u8; 16];
    let s = name.as_bytes();
    // An unmatched prefix leaves the retail index uninitialised; 0 here.
    let (prefix, used) = longest(s, &PREFIXES).unwrap_or((0, 0));
    id[0] = prefix as u8;
    // Retail steps one byte past the match (the `_`), also when nothing matched.
    let rest = skip(s, used);
    match prefix {
        1 => {
            // An unmatched kind keeps the prefix index in the retail local (1 = cap).
            let (kind, used) = longest(rest, &KINDS).unwrap_or((1, 0));
            id[5] = kind as u8;
            let rest = location(&mut id, skip(rest, used));
            let (instance, n) = number(rest);
            id[6] = instance as u8;
            let rest = subset(&mut id, skip(rest, n));
            let (index, _) = number(rest);
            id[14..16].copy_from_slice(&(index as u16).to_be_bytes());
        }
        3 => {
            let rest = location(&mut id, rest);
            let (instance, n) = number(rest);
            id[6] = instance as u8;
            let (index, _) = number(skip(rest, n));
            id[14..16].copy_from_slice(&(index as u16).to_be_bytes());
        }
        _ => {}
    }
    id
}

/// `sub_82457780`: id bytes 1..4 = the first four bytes of the location token (`ERR3` when no token follows it).
fn location<'a>(id: &mut [u8; 16], s: &'a [u8]) -> &'a [u8] {
    if !has_next(s) {
        id[1..5].copy_from_slice(b"ERR3");
        return s;
    }
    let (t, next) = token(s);
    for (i, b) in t.iter().take(4).enumerate() {
        id[1 + i] = *b;
    }
    &s[next..]
}

/// `sub_82457850`: id bytes 7.. = the subset token when a further token follows it, else empty (the index follows).
fn subset<'a>(id: &mut [u8; 16], s: &'a [u8]) -> &'a [u8] {
    if !has_next(s) {
        return s;
    }
    let (t, next) = token(s);
    // Retail copies up to 31 bytes from +7 and then writes the index over +14..+15: 7 bytes fit.
    for (i, b) in t.iter().take(7).enumerate() {
        id[7 + i] = *b;
    }
    &s[next..]
}

fn c_str(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// `sub_82457620`: does `line` (a path id) pass `filter`?
pub fn matches(line: &[u8; 16], filter: &[u8; 16]) -> bool {
    let index = |b: &[u8; 16]| i16::from_be_bytes([b[14], b[15]]);
    if line[0] == 0 {
        return if filter[0] == 0 { line == filter } else { matches!(filter[0], 4 | 5) };
    }
    if filter[0] == ANY_LINE {
        return true;
    }
    if filter[0] != line[0] {
        return false;
    }
    match line[0] {
        1 => {
            (filter[5] == ANY_KIND || filter[5] == line[5])
                && (filter[6] == 0 || filter[6] == line[6])
                && (filter[7] == b'*' || c_str(&filter[7..14]) == c_str(&line[7..14]))
                && (index(filter) == -1 || index(filter) == index(line))
        }
        3 => (filter[6] == 0 || filter[6] == line[6]) && (index(filter) == -1 || index(filter) == index(line)),
        _ => true,
    }
}

/// A row's `tRoundPaths` as exported: the names, the FilterDisabled flag (+0x320 == 1) and AllowDynamic (+0x324).
#[derive(Clone, Debug, PartialEq)]
pub struct RoundPaths {
    pub names: Vec<String>,
    pub all_paths: bool,
    pub param: u32,
}

impl Default for RoundPaths {
    fn default() -> Self {
        Self { names: Vec::new(), all_paths: true, param: 0 }
    }
}

impl RoundPaths {
    /// The ids of `lines` a rider may take, sorted (for `BranchContext::valid_lines`); None = no filter (FilterDisabled,
    /// or no names: what retail's subset list does when empty is not read [open], taken as no filter).
    pub fn valid_lines<'a>(&self, lines: impl IntoIterator<Item = &'a [u8; 16]>) -> Option<Vec<[u8; 16]>> {
        if self.all_paths || self.names.is_empty() {
            return None;
        }
        let filters: Vec<[u8; 16]> = self.names.iter().map(|n| filter_id(n)).collect();
        let mut out: Vec<[u8; 16]> = lines.into_iter().filter(|l| filters.iter().any(|f| matches(l, f))).copied().collect();
        out.sort_unstable();
        out.dedup();
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: u8, loc: &[u8; 4], instance: u8, subset: &str, index: u16) -> [u8; 16] {
        let mut id = [0u8; 16];
        id[0] = 1;
        id[1..5].copy_from_slice(loc);
        id[5] = kind;
        id[6] = instance;
        id[7..7 + subset.len()].copy_from_slice(subset.as_bytes());
        id[14..16].copy_from_slice(&index.to_be_bytes());
        id
    }

    #[test]
    fn names_parse_like_retail() {
        assert_eq!(filter_id("ch_race_indu_01_y_0"), line(12, b"indu", 1, "y", 0));
        assert_eq!(filter_id("ch_race_univ_01_A_5"), line(12, b"univ", 1, "A", 5));
        // No subset token when the index is the last one.
        assert_eq!(filter_id("ch_race_univ_01_3"), line(12, b"univ", 1, "", 3));
        assert_eq!(filter_id("ch_street_dwtn_01_AB_12"), line(15, b"dwtn", 1, "AB", 12));
        assert_eq!(filter_id("ch_photo_univ_02_3"), line(11, b"univ", 2, "", 3));
        // The longest kind wins ("caf" vs "cap" differ; "ots" vs "otl").
        assert_eq!(filter_id("ch_ots_dwtn_02_0")[5], 10);
    }

    #[test]
    fn lines_match_by_kind_instance_subset_and_index() {
        let f = filter_id("ch_race_indu_01_y_3");
        assert!(matches(&line(12, b"indu", 1, "y", 3), &f));
        // Location bytes are not compared.
        assert!(matches(&line(12, b"dwtn", 1, "y", 3), &f));
        assert!(!matches(&line(12, b"indu", 1, "y", 4), &f));
        assert!(!matches(&line(12, b"indu", 1, "z", 3), &f));
        assert!(!matches(&line(12, b"indu", 2, "y", 3), &f));
        assert!(!matches(&line(15, b"indu", 1, "y", 3), &f));
        let mut ambient = [0u8; 16];
        ambient[3] = 9;
        assert!(!matches(&ambient, &f));
        let mut wild = f;
        wild[7] = b'*';
        wild[14..16].copy_from_slice(&(-1i16).to_be_bytes());
        assert!(matches(&line(12, b"indu", 1, "q", 9), &wild));
    }

    #[test]
    fn round_paths_pick_the_named_lines() {
        let lines = [line(15, b"dwtn", 1, "", 0), line(15, b"dwtn", 1, "AA", 0), line(15, b"dwtn", 1, "AA", 1), line(15, b"dwtn", 1, "B", 0)];
        let r = RoundPaths { names: vec!["ch_street_dwtn_01_AA_1".into(), "ch_street_dwtn_01_0".into()], all_paths: false, param: 0 };
        let mut want = vec![lines[0], lines[2]];
        want.sort_unstable();
        assert_eq!(r.valid_lines(lines.iter()), Some(want));
        assert_eq!(RoundPaths { all_paths: true, ..r.clone() }.valid_lines(lines.iter()), None);
    }
}
