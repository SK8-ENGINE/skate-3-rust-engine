//! Csis project (`.csi`, magic `MOIR`): the symbol tables game code and banks bind to.
//! Table 0 = Functions (`*_msg`), 1 = Classes (`c_*`), 2 = GlobalVariables (with a default value).
use super::{FormatError, err};
use crate::be::{cstr, i32_at, u16_at, u32_at};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub name_id: u16,
    /// GlobalVariables only: the default value.
    pub default: i32,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub name: String,
    pub id: u16,
    /// [functions, classes, globals]
    pub tables: [Vec<Symbol>; 3],
}

impl Project {
    pub fn parse(name: &str, d: &[u8]) -> Result<Self, FormatError> {
        if d.len() < 0x28 || &d[..4] != b"MOIR" {
            return err(format!("{name}: not a MOIR project"));
        }
        let counts = [u16_at(d, 0x0A), u16_at(d, 0x0C), u16_at(d, 0x0E)];
        let id = u16_at(d, 0x10);
        let mut at = 0x28usize;
        let mut tables: [Vec<Symbol>; 3] = Default::default();
        for (t, &count) in counts.iter().enumerate() {
            let stride = if t == 2 { 16 } else { 12 };
            for i in 0..count as usize {
                let r = at + stride * i;
                if r + stride > d.len() {
                    return err(format!("{name}: table {t} runs past the file"));
                }
                let (name_off, name_id, default) = if t == 2 {
                    (u32_at(d, r + 8), u16_at(d, r + 12), i32_at(d, r + 4))
                } else {
                    (u32_at(d, r + 4), u16_at(d, r + 8), 0)
                };
                if name_off as usize >= d.len() {
                    return err(format!("{name}: symbol name outside the file"));
                }
                tables[t].push(Symbol { name: cstr(d, name_off as usize), name_id, default });
            }
            at += stride * count as usize;
        }
        Ok(Self { name: name.to_string(), id, tables })
    }
}
