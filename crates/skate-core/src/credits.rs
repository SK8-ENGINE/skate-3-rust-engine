//! The credits: retail FE 25 (template 30, b489), lines from `credits.txt` via `FECredits`; mods append lines.
//! Retail flow: docs/hails-additions/31-retail-menus.md#credits-screen

/// The movie state the screen posts once the credits are loaded (82608CE8).
pub const STATE_ROLLING: i32 = 1;

/// One credit line (retail entry 0x2C bytes "credit-entry": text +0, font name +0x10, size +0x20, offset +0x24,
/// spacing +0x28).
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// `#ID_...` (localised by GetLine) or literal text (kept with its leading `#`, APT's literal marker).
    pub text: String,
    /// Font table index (0x82FCA4BC: every slot "Futura Std Medium"); stored, no native returns it.
    pub font: i32,
    pub size: i32,
    /// Line advance as a multiple of the field's text height.
    pub spacing: f32,
}

/// The file's text: UTF-8 when it is valid UTF-8, else one character per byte (the retail file is Latin-1: accented
/// names such as "González" are single bytes).
pub fn decode(bytes: &[u8]) -> String {
    std::str::from_utf8(bytes).map_or_else(|_| bytes.iter().map(|&b| char::from(b)).collect(), str::to_owned)
}

/// Parse a credits file (82D07C40). Lines that do not read as `<int> <int> <float> "<text>"` are skipped. The text
/// splits at the two-character `\n`, at `^` and at real newlines; when the first piece starts with `#`, the later ones
/// get a `#` too; empty pieces are dropped.
pub fn parse(source: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        let mut head = line.splitn(4, char::is_whitespace);
        let (Some(font), Some(size), Some(spacing), Some(rest)) = (head.next(), head.next(), head.next(), head.next()) else {
            continue;
        };
        let (Ok(font), Ok(size), Ok(spacing)) = (font.parse::<i32>(), size.parse::<i32>(), spacing.parse::<f32>()) else {
            continue;
        };
        let rest = rest.trim();
        let Some(text) = rest.strip_prefix('"').and_then(|r| r.rfind('"').map(|end| &r[..end])) else { continue };
        let literal = text.starts_with('#');
        for (i, piece) in text.replace("\\n", "\n").split(['\n', '^']).enumerate() {
            if piece.is_empty() {
                continue;
            }
            let text = if i > 0 && literal && !piece.starts_with('#') { format!("#{piece}") } else { piece.to_owned() };
            out.push(Entry { text, font, size, spacing: if spacing.is_finite() { spacing } else { 0.0 } });
        }
    }
    out
}

/// The loaded credits: the retail lines, then each mod's section in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Credits {
    pub entries: Vec<Entry>,
}

impl Credits {
    pub fn new(retail: Vec<Entry>, mods: &[(String, Vec<Entry>)]) -> Self {
        let mut entries = retail;
        for (_, section) in mods {
            entries.extend(section.iter().cloned());
        }
        Self { entries }
    }

    /// GetNumCredits (825AAF68).
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// GetFontSize (825AAFA8); 0 out of range.
    pub fn font_size(&self, i: i64) -> i32 {
        self.get(i).map_or(0, |e| e.size)
    }

    /// GetLineSpacing (825AB000); 0 out of range.
    pub fn line_spacing(&self, i: i64) -> f32 {
        self.get(i).map_or(0.0, |e| e.spacing)
    }

    /// GetLineOffset (825AB058): always 0 in retail (the movie never asks).
    pub fn line_offset(&self, _i: i64) -> i32 {
        0
    }

    /// GetLine (825AB0B0): the text; `#ID_...` localised without its `#` (`localise` gives the id back when the
    /// language has none); anything else as stored; empty out of range.
    pub fn line(&self, i: i64, localise: impl Fn(&str) -> String) -> String {
        match self.get(i) {
            Some(e) if e.text.starts_with("#ID") => localise(&e.text[1..]),
            Some(e) => e.text.clone(),
            None => String::new(),
        }
    }

    fn get(&self, i: i64) -> Option<&Entry> {
        usize::try_from(i).ok().and_then(|i| self.entries.get(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail's line shape (font 2, spacing 2.00, `#ID_` ids and literal names), the splits and the `#` carry, the
    /// blank spacer line `#`, and lines that do not parse.
    #[test]
    fn credits_parse_like_retail() {
        let source = "2 24 2.00 \"#ID_CREDITS_TITLE\"\n\
                      2 18 2.00 \"#First Name^Second Name\"\n\
                      2 18 2.00 \"#One\\nTwo\"\n\
                      2 18 2.00 \"#\"\n\
                      2 18 2.00 \"^\"\n\
                      garbage line\n\
                      2 x 2.00 \"#bad size\"\n\
                      2 30 1.50 \"plain\"\r\n";
        let entries = parse(source);
        let texts: Vec<&str> = entries.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["#ID_CREDITS_TITLE", "#First Name", "#Second Name", "#One", "#Two", "#", "plain"]);
        assert_eq!((entries[0].font, entries[0].size, entries[0].spacing), (2, 24, 2.0));
        assert_eq!((entries[6].size, entries[6].spacing), (30, 1.5));
    }

    /// The natives: ids localised without `#`, literal text as stored, out of range empty / 0, offset always 0;
    /// mod sections after the retail lines, in order.
    #[test]
    fn credits_answer_the_fecredits_natives() {
        let retail = parse("2 24 2.00 \"#ID_CREDITS_TITLE\"\n2 18 2.00 \"#Someone\"\n");
        let modded = vec![("mod_a".to_owned(), parse("2 20 1.00 \"#Mod Author\"\n"))];
        let credits = Credits::new(retail, &modded);
        let localise = |id: &str| if id == "ID_CREDITS_TITLE" { "Credits".to_owned() } else { id.to_owned() };
        assert_eq!(credits.count(), 3);
        assert_eq!(credits.line(0, localise), "Credits");
        assert_eq!(credits.line(1, localise), "#Someone");
        assert_eq!(credits.line(2, localise), "#Mod Author");
        assert_eq!((credits.font_size(2), credits.line_spacing(2)), (20, 1.0));
        assert_eq!((credits.line(3, localise), credits.font_size(-1), credits.line_spacing(9)), (String::new(), 0, 0.0));
        assert_eq!(credits.line_offset(0), 0);
    }

    /// The retail file is Latin-1; UTF-8 files read as UTF-8.
    #[test]
    fn credits_decode_latin1_and_utf8() {
        assert_eq!(decode(b"Irene Gonz\xe1lez"), "Irene González");
        assert_eq!(decode("González".as_bytes()), "González");
    }
}
